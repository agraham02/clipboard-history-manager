use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use global_hotkey::hotkey::HotKey;
use global_hotkey::{GlobalHotKeyEvent, HotKeyState};
use tray_icon::{MouseButtonState, TrayIconEvent};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId, WindowLevel};

use crate::history::ClipboardHistory;
use crate::tray::TrayState;
use crate::ui::PickerState;

const HOTKEY_DEBOUNCE_MS: u128 = 250;
const FOCUS_GRACE_MS: u128 = 400;
const POPUP_WIDTH: u32 = 760;
const POPUP_HEIGHT: u32 = 480;

pub struct App {
    pub tray: Option<TrayState>,
    pub history: Arc<Mutex<ClipboardHistory>>,
    pub dirty_flag: Arc<AtomicBool>,
    pub open_picker_hotkey: HotKey,
    _hotkey_manager: Option<global_hotkey::GlobalHotKeyManager>,
    last_hotkey_at: Option<Instant>,

    // Long-lived GPU context (survives across open/close)
    gpu: Option<GpuContext>,

    // Popup window + per-window state
    popup_window: Option<Arc<Window>>,
    opened_at: Option<Instant>,
    egui_ctx: egui::Context,
    egui_state: Option<egui_winit::State>,
    egui_renderer: Option<egui_wgpu::Renderer>,
    surface_state: Option<SurfaceState>,
    picker_state: PickerState,
}

/// Long-lived GPU resources cached across popup open/close cycles.
struct GpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

/// Per-window surface state, recreated each time the popup opens.
struct SurfaceState {
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
}

impl App {
    pub fn new(
        history: Arc<Mutex<ClipboardHistory>>,
        dirty_flag: Arc<AtomicBool>,
        hotkey: HotKey,
    ) -> Self {
        Self {
            tray: None,
            history,
            dirty_flag,
            open_picker_hotkey: hotkey,
            _hotkey_manager: None,
            last_hotkey_at: None,
            gpu: None,
            popup_window: None,
            opened_at: None,
            egui_ctx: egui::Context::default(),
            egui_state: None,
            egui_renderer: None,
            surface_state: None,
            picker_state: PickerState::new(),
        }
    }

    pub fn set_hotkey_manager(&mut self, manager: global_hotkey::GlobalHotKeyManager) {
        self._hotkey_manager = Some(manager);
    }

    fn ensure_gpu(&mut self) {
        if self.gpu.is_some() {
            return;
        }
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::METAL | wgpu::Backends::VULKAN,
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            ..Default::default()
        }))
        .expect("No suitable GPU adapter");
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("egui_device"),
                ..Default::default()
            },
            None,
        ))
        .expect("Failed to create wgpu device");
        self.gpu = Some(GpuContext { instance, adapter, device, queue });
    }

    fn open_popup(&mut self, event_loop: &ActiveEventLoop) {
        if self.popup_window.is_some() {
            return;
        }

        self.ensure_gpu();

        let attrs = Window::default_attributes()
            .with_title("Clipboard History")
            .with_inner_size(LogicalSize::new(POPUP_WIDTH, POPUP_HEIGHT))
            .with_resizable(false)
            .with_decorations(true)
            .with_window_level(WindowLevel::AlwaysOnTop);

        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                eprintln!("Failed to create popup window: {e}");
                return;
            }
        };

        // On macOS, allow the window to appear over fullscreen spaces.
        #[cfg(target_os = "macos")]
        {
            use winit::raw_window_handle::HasWindowHandle;
            use winit::raw_window_handle::RawWindowHandle;
            if let Ok(handle) = window.window_handle() {
                if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
                    unsafe {
                        use objc2_app_kit::{NSApplication, NSView, NSWindowCollectionBehavior};
                        use objc2::MainThreadMarker;

                        // The handle provides ns_view; get the window from it.
                        let ns_view_ptr = appkit.ns_view.as_ptr() as *mut NSView;
                        let ns_view = &*ns_view_ptr;
                        if let Some(ns_window) = ns_view.window() {
                            ns_window.setCollectionBehavior(
                                NSWindowCollectionBehavior::CanJoinAllSpaces
                                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
                            );
                            // Force the window to become key and front-most.
                            ns_window.makeKeyAndOrderFront(None);
                        }

                        // Activate the app so it comes to the foreground reliably.
                        let mtm = MainThreadMarker::new_unchecked();
                        let app = NSApplication::sharedApplication(mtm);
                        #[allow(deprecated)]
                        app.activateIgnoringOtherApps(true);
                    }
                }
            }
        }
        // Ordering the window to front helps when called from a background process.
        window.focus_window();

        // Create surface for this window (GPU context already cached)
        let gpu = self.gpu.as_ref().unwrap();
        let surface = gpu.instance.create_surface(window.clone()).unwrap();

        let size = window.inner_size();
        let surface_config = surface
            .get_default_config(&gpu.adapter, size.width.max(1), size.height.max(1))
            .expect("Surface not supported");
        surface.configure(&gpu.device, &surface_config);

        let egui_renderer = egui_wgpu::Renderer::new(&gpu.device, surface_config.format, None, 1, false);

        // Fresh context each open so the renderer and context texture state stay in sync.
        self.egui_ctx = egui::Context::default();

        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        self.surface_state = Some(SurfaceState {
            surface,
            surface_config,
        });
        self.egui_renderer = Some(egui_renderer);
        self.egui_state = Some(egui_state);
        self.popup_window = Some(window);
        self.opened_at = Some(Instant::now());
        self.picker_state.reset();
    }

    fn close_popup(&mut self) {
        self.popup_window = None;
        self.opened_at = None;
        self.egui_state = None;
        self.egui_renderer = None;
        self.surface_state = None;
        self.picker_state.reset();
    }

    fn render_egui(&mut self) {
        let Some(window) = &self.popup_window else { return };
        let Some(egui_state) = &mut self.egui_state else { return };
        let Some(renderer) = &mut self.egui_renderer else { return };
        let Some(surf) = &mut self.surface_state else { return };
        let Some(gpu) = &self.gpu else { return };

        let raw_input = egui_state.take_egui_input(window);
        let full_output = self.egui_ctx.run(raw_input, |ctx| {
            crate::ui::render_picker(ctx, &self.history, &self.dirty_flag, &mut self.picker_state);
        });

        egui_state.handle_platform_output(window, full_output.platform_output);

        let clipped_prims = self
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surf.surface_config.width, surf.surface_config.height],
            pixels_per_point: full_output.pixels_per_point,
        };

        for (id, delta) in &full_output.textures_delta.set {
            renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
        }

        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("egui_enc") });

        renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &clipped_prims,
            &screen_descriptor,
        );

        let surface_texture = match surf.surface.get_current_texture() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("Surface texture error: {e}");
                return;
            }
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("egui_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.118,
                        g: 0.118,
                        b: 0.118,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            ..Default::default()
        })
        .forget_lifetime();

        renderer.render(&mut render_pass, &clipped_prims, &screen_descriptor);
        drop(render_pass);

        gpu.queue.submit(std::iter::once(encoder.finish()));
        surface_texture.present();

        for id in &full_output.textures_delta.free {
            renderer.free_texture(id);
        }

        if self.picker_state.should_close {
            // Defer close to avoid borrow issues
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Poll every 100ms so we always check for hotkey events even when no window is open.
        event_loop.set_control_flow(ControlFlow::wait_duration(std::time::Duration::from_millis(100)));
        if self.tray.is_none() {
            self.tray = crate::tray::create_tray();
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let is_popup = self.popup_window.as_ref().is_some_and(|w| w.id() == id);
        if !is_popup {
            return;
        }

        // Forward events to egui
        if let Some(egui_state) = &mut self.egui_state {
            if let Some(window) = &self.popup_window {
                let resp = egui_state.on_window_event(window, &event);
                if resp.repaint {
                    window.request_redraw();
                }
            }
        }

        match event {
            WindowEvent::CloseRequested => self.close_popup(),
            WindowEvent::RedrawRequested => {
                self.render_egui();
                if self.picker_state.should_close {
                    self.close_popup();
                }
            }
            WindowEvent::Focused(false) => {
                // Close popup when it loses focus, but not during the initial
                // activation grace period (macOS may briefly defocus when
                // bringing a background app to the front).
                let within_grace = self
                    .opened_at
                    .is_some_and(|t| t.elapsed().as_millis() < FOCUS_GRACE_MS);
                if !within_grace {
                    self.close_popup();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Process hotkey events
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.id == self.open_picker_hotkey.id() && ev.state == HotKeyState::Pressed {
                let debounce = self
                    .last_hotkey_at
                    .is_some_and(|t| t.elapsed().as_millis() < HOTKEY_DEBOUNCE_MS);
                if debounce {
                    continue;
                }
                self.last_hotkey_at = Some(Instant::now());

                if self.popup_window.is_some() {
                    self.close_popup();
                } else {
                    self.open_popup(event_loop);
                }
            }
        }

        // Process tray events
        while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
            if let TrayIconEvent::Click { button_state: MouseButtonState::Down, .. } = ev {
                // Left click on tray — toggle popup
                if self.popup_window.is_some() {
                    self.close_popup();
                } else {
                    self.open_popup(event_loop);
                }
            }
        }

        while let Ok(ev) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if let Some(tray) = &self.tray {
                if ev.id() == &tray.quit_menu_id {
                    event_loop.exit();
                } else if ev.id() == &tray.clear_menu_id {
                    if let Ok(mut h) = self.history.lock() {
                        h.clear();
                        self.dirty_flag.store(true, Ordering::Release);
                    }
                } else if ev.id() == &tray.show_menu_id {
                    if self.popup_window.is_none() {
                        self.open_popup(event_loop);
                    }
                }
            }
        }

        // Request continuous redraw while popup is open for smooth interaction.
        if let Some(w) = &self.popup_window {
            w.request_redraw();
        }
    }
}
