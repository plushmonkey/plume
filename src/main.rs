//#![windows_subsystem = "windows"]
use crate::{camera::Camera, map::Map};
use anyhow::*;
use std::sync::Arc;

use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

pub mod camera;
pub mod elvl;
pub mod map;
pub mod map_renderer;

enum Action {
    Drag(PhysicalPosition<f64>),
}

struct State {
    window: Arc<Window>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    size: winit::dpi::PhysicalSize<u32>,
    surface: wgpu::Surface<'static>,
    surface_format: wgpu::TextureFormat,
    map_renderer: map_renderer::MapRenderer,
    camera: Camera,
    mouse_position: PhysicalPosition<f64>,

    // TODO: This should probably be moved into some map editor structure.
    action: Option<Action>,
}

impl State {
    async fn new(window: Arc<Window>, map: Map) -> State {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions::default())
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await
            .unwrap();

        let size = window.inner_size();

        let surface = instance.create_surface(window.clone()).unwrap();
        let cap = surface.get_capabilities(&adapter);
        let surface_format = cap.formats[0].add_srgb_suffix();

        let mut map_renderer = map_renderer::MapRenderer::new(&device, &surface_format);

        map_renderer.set_map(&map, &queue);

        let camera = Camera::new(
            size.width as f32,
            size.height as f32,
            glam::Vec2::new(0.0, 0.0),
            1.0f32 / 16.0f32,
        );

        let mut state = State {
            window,
            device,
            queue,
            size,
            surface,
            surface_format,
            map_renderer,
            camera,
            mouse_position: PhysicalPosition::new(0.0, 0.0),
            action: None,
        };

        state.configure_surface();

        state
    }

    fn get_window(&self) -> &Window {
        &self.window
    }

    fn configure_surface(&mut self) -> bool {
        if self.size.width == 0 || self.size.height == 0 {
            return false;
        }

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: self.surface_format,
            view_formats: vec![self.surface_format],
            alpha_mode: wgpu::CompositeAlphaMode::Auto,
            width: self.size.width,
            height: self.size.height,
            desired_maximum_frame_latency: 2,
            // TODO: Enable vsync again once wgpu is updated to not have validation errors.
            present_mode: wgpu::PresentMode::Mailbox,
        };

        self.surface.configure(&self.device, &surface_config);

        return true;
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) -> bool {
        self.size = new_size;
        self.camera
            .set_surface_dimensions(new_size.width as f32, new_size.height as f32);
        return self.configure_surface();
    }

    fn render(&mut self) -> bool {
        if self.size.width == 0 || self.size.height == 0 {
            return false;
        }

        let surface_texture = self.surface.get_current_texture();

        if let Err(_) = surface_texture {
            return false;
        }

        let surface_texture = surface_texture.unwrap();

        let texture_view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor {
                format: Some(self.surface_format.add_srgb_suffix()),
                ..Default::default()
            });

        self.map_renderer.update(&self.camera, &self.queue);

        if let Some(action) = &self.action {
            match action {
                Action::Drag(position) => {
                    let dx = ((self.mouse_position.x - position.x) as f32) * self.camera.scale();
                    let dy = ((self.mouse_position.y - position.y) as f32) * self.camera.scale();

                    self.camera.position.x -= dx;
                    self.camera.position.y -= dy;

                    self.action = Some(Action::Drag(self.mouse_position));
                }
            }
        }

        let mut encoder = self.device.create_command_encoder(&Default::default());

        {
            let mut renderpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &texture_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            self.map_renderer.render(&mut renderpass);
        }

        self.queue.submit([encoder.finish()]);
        self.window.pre_present_notify();

        // TODO: There's a bug in wgpu 26.0.1 with fundamental semaphore usage that causes a validation error.
        // Update to at least 26.0.2 when it's released.
        surface_texture.present();

        return true;
    }
}

struct App {
    state: Option<State>,
    map: Option<Map>,
}

impl App {
    fn new(map: Map) -> App {
        App {
            state: None,
            map: Some(map),
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let window = Arc::new(
            event_loop
                .create_window(Window::default_attributes())
                .unwrap(),
        );

        let map = self.map.take().unwrap();
        let state = pollster::block_on(State::new(window.clone(), map));

        self.state = Some(state);

        window.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let app_state = self.state.as_mut().unwrap();

        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::RedrawRequested => {
                if app_state.render() {
                    app_state.get_window().request_redraw();
                }
            }
            WindowEvent::Resized(size) => {
                if app_state.resize(size) {
                    app_state.get_window().request_redraw();
                    event_loop.set_control_flow(ControlFlow::Poll);
                } else {
                    // Block until we get events again. Don't spin cpu while window is minimized.
                    event_loop.set_control_flow(ControlFlow::Wait);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                app_state.mouse_position = position;
            }
            WindowEvent::MouseInput { state, button, .. } => match button {
                winit::event::MouseButton::Left => match state {
                    winit::event::ElementState::Pressed => {
                        app_state.action = Some(Action::Drag(app_state.mouse_position));
                    }
                    winit::event::ElementState::Released => {
                        app_state.action = None;
                    }
                },
                _ => {}
            },
            WindowEvent::MouseWheel { delta, .. } => match delta {
                winit::event::MouseScrollDelta::LineDelta(_, dy) => {
                    const SCROLL_SPEED: f32 = 1.0 / 5.0;

                    let mut scale = app_state.camera.scale;
                    let mut old_scale = scale;

                    if old_scale == 0.0f32 {
                        old_scale = 0.01f32;
                    }

                    scale = scale - (scale * (dy * SCROLL_SPEED));

                    // Calculate world difference change and reposition the camera so we keep pointing at the same world tile in the new scale.
                    let old_world_pos = app_state.camera.unproject(glam::Vec2::new(
                        app_state.mouse_position.x as f32,
                        app_state.mouse_position.y as f32,
                    ));
                    let world_offset =
                        (old_world_pos - app_state.camera.position) * (1.0f32 / old_scale);

                    app_state.camera.position += world_offset * (old_scale - scale);
                    app_state.camera.set_scale(scale);
                }
                _ => {}
            },
            _ => (),
        }
    }
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let event_loop = EventLoop::new().unwrap();

    event_loop.set_control_flow(ControlFlow::Poll);

    let map = map::Map::load("test.lvl")?;

    let mut app = App::new(map);

    event_loop.run_app(&mut app).unwrap();

    Ok(())
}
