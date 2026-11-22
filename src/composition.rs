#![allow(unsafe_op_in_unsafe_fn)]

use crate::dcomp_blur::{
    BlurSpec, CALIBRATED_BLUR_PX, ColorMatrixEffect, GaussianBlurEffect, blur_px_to_sigma,
    build_d2d_rounded_path, optical_luminosity_clamp_matrix,
};
use std::cell::RefCell;
use std::ffi::c_void;
use std::mem::size_of;
use std::sync::atomic::{AtomicU64, Ordering};
use windows::Foundation::Numerics::{Matrix3x2, Vector2, Vector3};
use windows::Graphics::Effects::{IGraphicsEffect, IGraphicsEffectSource};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBrush, CompositionEffectBrush, CompositionEffectSourceParameter,
    CompositionStretch, CompositionSurfaceBrush, Compositor, ContainerVisual, SpriteVisual,
};
use windows::Win32::Foundation::{BOOL, E_INVALIDARG, FALSE, HMODULE, HWND, TRUE};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
    D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_FACTORY_TYPE_SINGLE_THREADED,
    ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_USE_HOSTBACKDROPBRUSH, DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_DEVICE_HUNG, DXGI_ERROR_DEVICE_REMOVED, DXGI_ERROR_DEVICE_RESET,
    DXGI_ERROR_DRIVER_INTERNAL_ERROR, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGISurface,
    IDXGISwapChain1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Foundation::D2DERR_RECREATE_TARGET;
use windows::Win32::System::WinRT::Composition::{ICompositorDesktopInterop, ICompositorInterop};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::core::{Interface, Result};

static SHARED_DCOMP_GEN: AtomicU64 = AtomicU64::new(1);

#[derive(Clone)]
struct SharedDComp {
    generation: u64,
    compositor: Compositor,
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    dxgi_factory: IDXGIFactory2,
    #[allow(dead_code)]
    d2d_factory: ID2D1Factory1,
    d2d_device: ID2D1Device,
}

thread_local! {
    static DISPATCHER_CONTROLLER: RefCell<Option<DispatcherQueueController>> = const { RefCell::new(None) };
    static SHARED_DCOMP: RefCell<Option<SharedDComp>> = const { RefCell::new(None) };
}

fn shared_dcomp() -> Result<SharedDComp> {
    SHARED_DCOMP.with_borrow_mut(|slot| {
        if let Some(shared) = slot.as_ref() {
            // BH-06: Proactively invalidate cached device if GPU was reset/removed
            // so DCompHost::new recovery never reuses a dead thread-local device.
            if unsafe { shared.d3d_device.GetDeviceRemovedReason() }.is_ok() {
                return Ok(shared.clone());
            }
            *slot = None;
        }
        let compositor = Compositor::new()?;
        let (d3d_device, d3d_context) = create_d3d11_device()?;
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let dxgi_factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };

        let d2d_factory: ID2D1Factory1 = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)? };
        let d2d_device: ID2D1Device = unsafe { d2d_factory.CreateDevice(&dxgi_device)? };

        let generation = SHARED_DCOMP_GEN.fetch_add(1, Ordering::Relaxed);
        let shared = SharedDComp {
            generation,
            compositor,
            d3d_device,
            d3d_context,
            dxgi_factory,
            d2d_factory,
            d2d_device,
        };
        *slot = Some(shared.clone());
        Ok(shared)
    })
}

/// BH-33: Only evict `SHARED_DCOMP` when its generation still matches the failing host,
/// preventing a stale second window from evicting another window's newly recovered device.
fn clear_shared_if_device_lost_err(failed_gen: u64, err: &windows::core::Error) {
    if is_device_lost_hresult(err.code()) {
        SHARED_DCOMP.with_borrow_mut(|slot| {
            if slot.as_ref().is_some_and(|s| s.generation == failed_gen) {
                *slot = None;
            }
        });
    }
}

fn clear_shared_if_device_lost_hr(failed_gen: u64, hr: windows::core::HRESULT) -> bool {
    if is_device_lost_hresult(hr) {
        SHARED_DCOMP.with_borrow_mut(|slot| {
            if slot.as_ref().is_some_and(|s| s.generation == failed_gen) {
                *slot = None;
            }
        });
        true
    } else {
        false
    }
}

pub fn ensure_dispatcher_queue() -> Result<()> {
    DISPATCHER_CONTROLLER.with_borrow_mut(|slot| {
        if slot.is_some() {
            return Ok(());
        }
        unsafe {
            let opts = DispatcherQueueOptions {
                dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                threadType: DQTYPE_THREAD_CURRENT,
                apartmentType: DQTAT_COM_NONE,
            };
            let ctrl = CreateDispatcherQueueController(opts)?;
            *slot = Some(ctrl);
            Ok(())
        }
    })
}

pub fn release_thread_locals() {
    SHARED_DCOMP.with_borrow_mut(|slot| {
        *slot = None;
    });
    DISPATCHER_CONTROLLER.with_borrow_mut(|slot| {
        if let Some(ctrl) = slot.take() {
            let _ = ctrl.ShutdownQueueAsync();
        }
    });
}

fn create_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let levels = [
        D3D_FEATURE_LEVEL_11_1,
        D3D_FEATURE_LEVEL_11_0,
        D3D_FEATURE_LEVEL_10_0,
    ];
    for driver in [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP] {
        let mut device = None;
        let mut context = None;
        unsafe {
            let res = D3D11CreateDevice(
                None,
                driver,
                HMODULE::default(),
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                Some(&levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            );
            if res.is_ok()
                && let (Some(d), Some(c)) = (device, context)
            {
                return Ok((d, c));
            }
        }
    }
    Err(windows::core::Error::from(E_INVALIDARG))
}

/// Hardware-accelerated DirectComposition + DXGI SwapChain presenter for
/// premultiplied per-pixel alpha windows without DWM backdrop blur or GDI
/// `UpdateLayeredWindow` CPU sync overhead.
struct BlurLayer {
    visual: SpriteVisual,
    brush: CompositionEffectBrush,
    luminosity_clamped: bool,
    mask_swap_chain: IDXGISwapChain1,
    _mask_surface_brush: CompositionSurfaceBrush,
    mask_w: u32,
    mask_h: u32,
}

pub struct DCompHost {
    shared_gen: u64,
    _compositor: Compositor,
    _target: DesktopWindowTarget,
    root_visual: ContainerVisual,
    blur_layer: Option<BlurLayer>,
    content_visual: SpriteVisual,
    _d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain1,
    d2d_factory: ID2D1Factory1,
    d2d_context: ID2D1DeviceContext,
    _surface_brush: CompositionSurfaceBrush,
    width: u32,
    height: u32,
    bgra_scratch: Vec<u8>,
    prev_dirty: Option<(u32, u32, u32, u32)>,
    d2d_target: Option<ID2D1Bitmap1>,
    last_blur: Option<BlurSpec>,
}

fn init_blur_layer(
    compositor: &Compositor,
    comp_interop: &ICompositorInterop,
    factory: &IDXGIFactory2,
    d3d_device: &ID3D11Device,
    w: u32,
    h: u32,
) -> Result<BlurLayer> {
    let host_backdrop = compositor.CreateHostBackdropBrush()?;
    let source_param =
        CompositionEffectSourceParameter::Create(&windows::core::HSTRING::from("Backdrop"))?;
    let source_iface: IGraphicsEffectSource = source_param.cast()?;
    let calibrated_sigma = blur_px_to_sigma(CALIBRATED_BLUR_PX);
    let anim_props: windows::Foundation::Collections::IIterable<windows::core::HSTRING> =
        vec![windows::core::HSTRING::from("Blur.BlurAmount")].try_into()?;

    // Chain GaussianBlurEffect(24px) -> Option B Optical Luminosity Clamp (ColorMatrixEffect)
    // so pure #FFFFFF PDFs/light apps stay smoked obsidian (~10.8:1 AAA) with 0% CPU capture.
    // Falls back gracefully to GaussianBlur-only if the OS compositor rejects the chained graph.
    let blur_for_chain = GaussianBlurEffect::new(calibrated_sigma, Some(source_iface.clone()));
    let blur_src: IGraphicsEffectSource = blur_for_chain.into();
    let clamped_effect = ColorMatrixEffect::new(optical_luminosity_clamp_matrix(), Some(blur_src));
    let clamped_iface: IGraphicsEffect = clamped_effect.into();
    let (eff_factory, luminosity_clamped) =
        match compositor.CreateEffectFactoryWithProperties(&clamped_iface, &anim_props) {
            Ok(f)
                if !matches!(
                    f.LoadStatus(),
                    Ok(windows::UI::Composition::CompositionEffectFactoryLoadStatus::EffectTooComplex
                        | windows::UI::Composition::CompositionEffectFactoryLoadStatus::Other)
                ) =>
            {
                (f, true)
            }
            _ => {
                let fallback_blur = GaussianBlurEffect::new(calibrated_sigma, Some(source_iface));
                let fallback_iface: IGraphicsEffect = fallback_blur.into();
                (
                    compositor.CreateEffectFactoryWithProperties(&fallback_iface, &anim_props)?,
                    false,
                )
            }
        };
    let brush = eff_factory.CreateBrush()?;
    let backdrop_brush: CompositionBrush = host_backdrop.cast()?;
    brush.SetSourceParameter(&windows::core::HSTRING::from("Backdrop"), &backdrop_brush)?;

    let mw = w.max(1);
    let mh = h.max(1);
    let mask_desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: mw,
        Height: mh,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        Stereo: FALSE,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        Scaling: DXGI_SCALING_STRETCH,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Flags: 0,
    };
    let mask_swap_chain = unsafe { factory.CreateSwapChainForComposition(d3d_device, &mask_desc, None)? };
    let mask_surface = unsafe { comp_interop.CreateCompositionSurfaceForSwapChain(&mask_swap_chain)? };
    let mask_surface_brush = compositor.CreateSurfaceBrushWithSurface(&mask_surface)?;
    mask_surface_brush.SetStretch(CompositionStretch::None)?;

    let mask_brush = compositor.CreateMaskBrush()?;
    let effect_brush_base: CompositionBrush = brush.cast()?;
    let mask_brush_base: CompositionBrush = mask_surface_brush.cast()?;
    mask_brush.SetSource(&effect_brush_base)?;
    mask_brush.SetMask(&mask_brush_base)?;

    let visual = compositor.CreateSpriteVisual()?;
    visual.SetIsVisible(false)?;
    let combined_brush: CompositionBrush = mask_brush.cast()?;
    visual.SetBrush(&combined_brush)?;
    Ok(BlurLayer {
        visual,
        brush,
        luminosity_clamped,
        mask_swap_chain,
        _mask_surface_brush: mask_surface_brush,
        mask_w: mw,
        mask_h: mh,
    })
}

impl DCompHost {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        ensure_dispatcher_queue()?;
        let w = width.max(1);
        let h = height.max(1);

        // Enable DWM host-backdrop sampling on Win32 WS_POPUP + WS_EX_NOREDIRECTIONBITMAP
        // windows so Compositor::CreateHostBackdropBrush() composites the desktop behind
        // the window instead of rendering opaque black.
        unsafe {
            let margins = MARGINS {
                cxLeftWidth: -1,
                cxRightWidth: -1,
                cyTopHeight: -1,
                cyBottomHeight: -1,
            };
            let _ = DwmExtendFrameIntoClientArea(hwnd, &margins);
            let use_host_backdrop = TRUE;
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_USE_HOSTBACKDROPBRUSH,
                &use_host_backdrop as *const BOOL as *const c_void,
                size_of::<BOOL>() as u32,
            );
        }

        let SharedDComp {
            generation: shared_gen,
            compositor,
            d3d_device,
            d3d_context,
            dxgi_factory: factory,
            d2d_factory,
            d2d_device,
        } = shared_dcomp()?;
        let desktop_interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { desktop_interop.CreateDesktopWindowTarget(hwnd, false)? };

        let d2d_context = unsafe {
            d2d_device.CreateDeviceContext(windows::Win32::Graphics::Direct2D::D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?
        };

        let root_visual = compositor.CreateContainerVisual()?;
        root_visual.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        target.SetRoot(&root_visual)?;

        // D3D11 + DXGI SwapChainForComposition (PREMULTIPLIED RGBA->BGRA upload)
        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w,
            Height: h,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            Stereo: FALSE,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            Scaling: DXGI_SCALING_STRETCH,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
            Flags: 0,
        };
        let swap_chain =
            unsafe { factory.CreateSwapChainForComposition(&d3d_device, &desc, None)? };

        let comp_interop: ICompositorInterop = compositor.cast()?;
        let surface = unsafe { comp_interop.CreateCompositionSurfaceForSwapChain(&swap_chain)? };
        let surface_brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
        surface_brush.SetStretch(CompositionStretch::Fill)?;

        let blur_layer = match init_blur_layer(&compositor, &comp_interop, &factory, &d3d_device, w, h) {
            Ok(layer) => {
                let _ = root_visual.Children()?.InsertAtTop(&layer.visual);
                Some(layer)
            }
            Err(_) => None,
        };

        let content_visual = compositor.CreateSpriteVisual()?;
        content_visual.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        let surface_brush_base: CompositionBrush = surface_brush.cast()?;
        content_visual.SetBrush(&surface_brush_base)?;
        root_visual.Children()?.InsertAtTop(&content_visual)?;

        let mut host = Self {
            shared_gen,
            _compositor: compositor,
            _target: target,
            root_visual,
            blur_layer,
            content_visual,
            _d3d_device: d3d_device,
            d3d_context,
            swap_chain,
            d2d_factory,
            d2d_context,
            _surface_brush: surface_brush,
            width: w,
            height: h,
            bgra_scratch: vec![0u8; (w as usize) * (h as usize) * 4],
            prev_dirty: None,
            d2d_target: None,
            last_blur: None,
        };
        host.bind_d2d_target()?;
        Ok(host)
    }

    fn bind_d2d_target(&mut self) -> Result<()> {
        let gen_id = self.shared_gen;
        unsafe {
            let surface: IDXGISurface = self.swap_chain.GetBuffer(0).inspect_err(|e| {
                clear_shared_if_device_lost_err(gen_id, e);
            })?;
            let props = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                colorContext: std::mem::ManuallyDrop::new(None),
            };
            let bitmap: ID2D1Bitmap1 = self
                .d2d_context
                .CreateBitmapFromDxgiSurface(&surface, Some(&props))
                .inspect_err(|e| {
                    clear_shared_if_device_lost_err(gen_id, e);
                })?;
            self.d2d_context.SetTarget(&bitmap);
            self.d2d_target = Some(bitmap);
        }
        Ok(())
    }

    pub fn d2d_context(&self) -> &ID2D1DeviceContext {
        &self.d2d_context
    }

    pub fn blur_supported(&self) -> bool {
        self.blur_layer.is_some()
    }

    /// True when the WinRT `GaussianBlurEffect -> ColorMatrixEffect` Option B
    /// Optical Luminosity Clamp chain loaded without falling back to blur-only.
    pub fn luminosity_clamp_active(&self) -> bool {
        self.blur_layer
            .as_ref()
            .is_some_and(|l| l.luminosity_clamped)
    }

    /// Updates the single-HWND native DirectComposition backdrop blur layer using
    /// a Direct2D-antialiased `CompositionMaskBrush` alpha mask.
    ///
    /// Must be called outside `begin_d2d_draw()`..`end_d2d_draw_and_present()`.
    /// Returns `false` if host-backdrop blur is unavailable (caller should fall back
    /// to denser translucent glass).
    pub fn set_blur(&mut self, spec: Option<BlurSpec>) -> bool {
        let Some(layer) = self.blur_layer.as_mut() else {
            return false;
        };
        if self.last_blur == spec {
            return true;
        }
        let Some(s) = spec else {
            let _ = layer.visual.SetIsVisible(false);
            self.last_blur = None;
            return true;
        };

        let w = s.w.max(1.0);
        let h = s.h.max(1.0);
        let prev = self.last_blur;

        if prev.is_none_or(|p| p.blur_px != s.blur_px) {
            let sigma = blur_px_to_sigma(s.blur_px);
            if let Ok(props) = layer.brush.Properties() {
                let _ = props.InsertScalar(&windows::core::HSTRING::from("Blur.BlurAmount"), sigma);
            }
        }

        if prev.is_none_or(|p| {
            (p.w, p.h, p.radii, p.box1_h, p.extra_boxes)
                != (s.w, s.h, s.radii, s.box1_h, s.extra_boxes)
        }) {
            let mw = w.ceil().max(1.0) as u32;
            let mh = h.ceil().max(1.0) as u32;
            let ok = unsafe {
                let resized = if layer.mask_w != mw || layer.mask_h != mh {
                    layer
                        .mask_swap_chain
                        .ResizeBuffers(
                            2,
                            mw,
                            mh,
                            DXGI_FORMAT_B8G8R8A8_UNORM,
                            DXGI_SWAP_CHAIN_FLAG(0),
                        )
                        .map(|_| {
                            layer.mask_w = mw;
                            layer.mask_h = mh;
                        })
                } else {
                    Ok(())
                };
                let draw_res = resized.and_then(|()| {
                    let surface: IDXGISurface = layer.mask_swap_chain.GetBuffer(0)?;
                    let props = D2D1_BITMAP_PROPERTIES1 {
                        pixelFormat: D2D1_PIXEL_FORMAT {
                            format: DXGI_FORMAT_B8G8R8A8_UNORM,
                            alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                        },
                        dpiX: 96.0,
                        dpiY: 96.0,
                        bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW,
                        colorContext: std::mem::ManuallyDrop::new(None),
                    };
                    let mask_bmp: ID2D1Bitmap1 =
                        self.d2d_context.CreateBitmapFromDxgiSurface(&surface, Some(&props))?;
                    let primary_h = s.box1_h.unwrap_or(h).clamp(1.0, h);
                    let path = build_d2d_rounded_path(&self.d2d_factory, 0.0, 0.0, w, primary_h, s.radii)?;
                    let white = D2D1_COLOR_F {
                        r: 1.0,
                        g: 1.0,
                        b: 1.0,
                        a: 1.0,
                    };
                    let white_brush = self.d2d_context.CreateSolidColorBrush(&white, None)?;
                    self.d2d_context.SetTarget(&mask_bmp);
                    self.d2d_context.BeginDraw();
                    self.d2d_context.SetTransform(&Matrix3x2::identity());
                    self.d2d_context
                        .SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
                    self.d2d_context.Clear(Some(&D2D1_COLOR_F {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }));
                    self.d2d_context.FillGeometry(&path, &white_brush, None);
                    if let Some(extras) = s.extra_boxes {
                        for (bx, by, bw, bh, br) in extras {
                            if bw > 0.5 && bh > 0.5 {
                                let ep = build_d2d_rounded_path(
                                    &self.d2d_factory,
                                    bx,
                                    by,
                                    bw,
                                    bh,
                                    (br, br, br, br),
                                )?;
                                self.d2d_context.FillGeometry(&ep, &white_brush, None);
                            }
                        }
                    }
                    let end_res = self.d2d_context.EndDraw(None, None);
                    if let Some(t) = self.d2d_target.as_ref() {
                        self.d2d_context.SetTarget(t);
                    } else {
                        self.d2d_context.SetTarget(None);
                    }
                    end_res?;
                    let hr = layer.mask_swap_chain.Present(0, DXGI_PRESENT(0));
                    if hr.is_err() {
                        return Err(windows::core::Error::from(hr));
                    }
                    Ok(())
                });
                draw_res.is_ok()
            };
            if !ok {
                let _ = layer.visual.SetIsVisible(false);
                self.last_blur = None;
                return false;
            }
        }

        if prev.is_none_or(|p| (p.x, p.y) != (s.x, s.y)) {
            let _ = layer.visual.SetOffset(Vector3 {
                X: s.x,
                Y: s.y,
                Z: 0.0,
            });
        }
        if prev.is_none_or(|p| (p.w, p.h) != (s.w, s.h)) {
            let mw = w.ceil().max(1.0) as f32;
            let mh = h.ceil().max(1.0) as f32;
            let _ = layer.visual.SetSize(Vector2 { X: mw, Y: mh });
        }
        if prev.is_none() {
            let _ = layer.visual.SetIsVisible(true);
        }
        self.last_blur = Some(s);
        true
    }

    pub fn begin_d2d_draw(&mut self) -> Result<&ID2D1DeviceContext> {
        if self.d2d_target.is_none() {
            self.bind_d2d_target()?;
        }
        unsafe {
            self.d2d_context.BeginDraw();
            self.d2d_context
                .SetTransform(&windows::Foundation::Numerics::Matrix3x2::identity());
            self.d2d_context
                .SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            self.d2d_context.Clear(Some(&D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }));
        }
        Ok(&self.d2d_context)
    }

    pub fn end_d2d_draw_and_present(&mut self) -> Result<()> {
        let gen_id = self.shared_gen;
        unsafe {
            if let Err(e) = self.d2d_context.EndDraw(None, None) {
                clear_shared_if_device_lost_err(gen_id, &e);
                return Err(e);
            }
            let hr = self.swap_chain.Present(0, DXGI_PRESENT(0));
            if hr.is_err() {
                clear_shared_if_device_lost_hr(gen_id, hr);
                return Err(windows::core::Error::from(hr));
            }
        }
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return Ok(());
        }
        self.rebind_swap_chain(w, h)
    }

    /// Forces a clean D2D target release + DXGI `ResizeBuffers` + rebind (e.g. after `WM_DPICHANGED`)
    /// so no stale DXGI surface references leak across monitor DPI transitions.
    pub fn rebind_for_dpi_change(&mut self, width: u32, height: u32) -> Result<()> {
        self.rebind_swap_chain(width.max(1), height.max(1))
    }

    fn rebind_swap_chain(&mut self, w: u32, h: u32) -> Result<()> {
        let gen_id = self.shared_gen;
        unsafe {
            self.d2d_context.SetTarget(None);
            self.d2d_target = None;
            self.swap_chain
                .ResizeBuffers(
                    2,
                    w,
                    h,
                    DXGI_FORMAT_B8G8R8A8_UNORM,
                    DXGI_SWAP_CHAIN_FLAG(0),
                )
                .inspect_err(|e| {
                    clear_shared_if_device_lost_err(gen_id, e);
                })?;
        }
        self.width = w;
        self.height = h;
        let sz = Vector2 {
            X: w as f32,
            Y: h as f32,
        };
        self.root_visual.SetSize(sz)?;
        self.content_visual.SetSize(sz)?;
        let needed = (w as usize) * (h as usize) * 4;
        if self.bgra_scratch.len() != needed {
            self.bgra_scratch.resize(needed, 0);
            if needed < self.bgra_scratch.capacity() {
                self.bgra_scratch.shrink_to_fit();
            }
        }
        self.prev_dirty = None;
        self.bind_d2d_target()?;
        Ok(())
    }

    pub fn present_rgba_premultiplied(
        &mut self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<()> {
        self.present_rgba_premultiplied_rect(rgba_data, width, height, None)
    }

    pub fn present_rgba_premultiplied_rect(
        &mut self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
        dirty: Option<(u32, u32, u32, u32)>,
    ) -> Result<()> {
        if width != self.width || height != self.height {
            self.resize(width, height)?;
        }
        let expected = (self.width as usize) * (self.height as usize) * 4;
        if rgba_data.len() < expected {
            return Err(windows::core::Error::from(E_INVALIDARG));
        }

        let clamped_box = dirty.and_then(|(dx, dy, dw, dh)| {
            let left = dx.min(self.width);
            let top = dy.min(self.height);
            let right = dx.saturating_add(dw).min(self.width);
            let bottom = dy.saturating_add(dh).min(self.height);
            let cw = right.saturating_sub(left);
            let ch = bottom.saturating_sub(top);
            if cw > 0 && ch > 0 {
                Some((left, top, cw, ch))
            } else {
                None
            }
        });

        if let Some((dx, dy, dw, dh)) = clamped_box {
            let stride = self.width as usize;
            let row_bytes = (dw as usize) * 4;
            for row in dy..dy + dh {
                let off = ((row as usize) * stride + (dx as usize)) * 4;
                swizzle_rgba_to_bgra(
                    &mut self.bgra_scratch[off..off + row_bytes],
                    &rgba_data[off..off + row_bytes],
                );
            }
        } else {
            swizzle_rgba_to_bgra(&mut self.bgra_scratch[..expected], &rgba_data[..expected]);
        }

        let upload_box = match (clamped_box, self.prev_dirty) {
            (Some((dx, dy, dw, dh)), Some((px, py, pw, ph))) => {
                let x0 = dx.min(px);
                let y0 = dy.min(py);
                let x1 = dx.saturating_add(dw).max(px.saturating_add(pw)).min(self.width);
                let y1 = dy.saturating_add(dh).max(py.saturating_add(ph)).min(self.height);
                Some((x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)))
            }
            (Some(cur), None) => Some(cur),
            (None, _) => None,
        };
        self.prev_dirty = clamped_box;

        let gen_id = self.shared_gen;
        unsafe {
            let back_buf: ID3D11Texture2D = self.swap_chain.GetBuffer(0).inspect_err(|e| {
                clear_shared_if_device_lost_err(gen_id, e);
            })?;
            let resource: ID3D11Resource = back_buf.cast()?;
            let row_pitch = self.width * 4;
            let depth_pitch = expected as u32;
            if let Some((dx, dy, dw, dh)) = upload_box
                && dw > 0
                && dh > 0
            {
                let d3d_box = D3D11_BOX {
                    left: dx,
                    top: dy,
                    front: 0,
                    right: dx + dw,
                    bottom: dy + dh,
                    back: 1,
                };
                let src_offset = ((dy as usize) * (self.width as usize) + (dx as usize)) * 4;
                let src_ptr = self.bgra_scratch.as_ptr().add(src_offset) as *const core::ffi::c_void;
                self.d3d_context.UpdateSubresource(
                    &resource,
                    0,
                    Some(&d3d_box),
                    src_ptr,
                    row_pitch,
                    depth_pitch,
                );
            } else {
                self.d3d_context.UpdateSubresource(
                    &resource,
                    0,
                    None,
                    self.bgra_scratch.as_ptr() as *const core::ffi::c_void,
                    row_pitch,
                    depth_pitch,
                );
            }
            let hr = self.swap_chain.Present(0, DXGI_PRESENT(0));
            if hr.is_err() {
                clear_shared_if_device_lost_hr(gen_id, hr);
                return Err(windows::core::Error::from(hr));
            }
        }
        Ok(())
    }
}

impl Drop for DCompHost {
    fn drop(&mut self) {
        unsafe {
            self.d2d_context.SetTarget(None);
        }
        self.d2d_target = None;
        // BH-33: Explicitly close WinRT DesktopWindowTarget so HWND is immediately unbound
        // and can accept a fresh CreateDesktopWindowTarget during device-lost recovery.
        let _ = self._target.Close();
    }
}

fn is_device_lost_hresult(hr: windows::core::HRESULT) -> bool {
    hr == DXGI_ERROR_DEVICE_REMOVED
        || hr == DXGI_ERROR_DEVICE_RESET
        || hr == DXGI_ERROR_DEVICE_HUNG
        || hr == DXGI_ERROR_DRIVER_INTERNAL_ERROR
        || hr == D2DERR_RECREATE_TARGET
}

fn swizzle_rgba_to_bgra(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst
        .chunks_exact_mut(4)
        .zip(src.chunks_exact(4))
    {
        let px = u32::from_le_bytes(s.try_into().unwrap());
        d.copy_from_slice(&((px & 0xFF00_FF00) | ((px >> 16) & 0xFF) | ((px & 0xFF) << 16)).to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_device_lost_handling() {
        assert!(is_device_lost_hresult(DXGI_ERROR_DEVICE_REMOVED));
        assert!(is_device_lost_hresult(DXGI_ERROR_DEVICE_RESET));
        assert!(is_device_lost_hresult(DXGI_ERROR_DEVICE_HUNG));
        assert!(is_device_lost_hresult(DXGI_ERROR_DRIVER_INTERNAL_ERROR));
        assert!(is_device_lost_hresult(D2DERR_RECREATE_TARGET));
        assert!(!is_device_lost_hresult(windows::core::HRESULT(0)));

        // BH-33: Verify generation-guarded SHARED_DCOMP eviction does not clobber newer devices.
        let _ = ensure_dispatcher_queue();
        if let Ok(first) = shared_dcomp() {
            let gen1 = first.generation;
            // Stale generation must NOT evict current device
            clear_shared_if_device_lost_hr(gen1.wrapping_sub(1), DXGI_ERROR_DEVICE_RESET);
            let still_same = shared_dcomp().unwrap().generation;
            assert_eq!(still_same, gen1);
            // Matching generation MUST evict and force a new generation
            clear_shared_if_device_lost_hr(gen1, DXGI_ERROR_DEVICE_RESET);
            let gen2 = shared_dcomp().unwrap().generation;
            assert_ne!(gen2, gen1);
            release_thread_locals();
        }
    }
}
