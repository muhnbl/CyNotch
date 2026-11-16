#![allow(unsafe_op_in_unsafe_fn)]

use crate::models::{NotchPlacement, NotchStyle};
use std::cell::RefCell;
use windows::core::{implement, IInspectable, Interface, Result, GUID, HSTRING, PCWSTR};
use windows::Foundation::Numerics::Vector2;
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl,
};
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBrush, CompositionEffectBrush, CompositionEffectSourceParameter,
    CompositionRectangleGeometry, CompositionRoundedRectangleGeometry, CompositionStretch,
    CompositionSurfaceBrush, Compositor, ContainerVisual, SpriteVisual,
};
use windows::Win32::Foundation::{E_INVALIDARG, FALSE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIAdapter, IDXGIDevice, IDXGIFactory2, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};
use windows::Win32::System::WinRT::Composition::{ICompositorDesktopInterop, ICompositorInterop};
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl, GRAPHICS_EFFECT_PROPERTY_MAPPING,
};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DispatcherQueueOptions, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT,
};


// Direct2D GaussianBlur built-in effect CLSID: {1feb6d69-2fe6-4ac9-8c58-1d7f93e7a6a5}
const CLSID_D2D1_GAUSSIAN_BLUR: GUID = GUID::from_u128(0x1feb6d69_2fe6_4ac9_8c58_1d7f93e7a6a5);
// D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION = 0
// D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION = 1
// D2D1_GAUSSIANBLUR_PROP_BORDER_MODE = 2 (D2D1_BORDER_MODE_SOFT = 0, D2D1_BORDER_MODE_HARD = 1)

thread_local! {
    static DISPATCHER_CONTROLLER: RefCell<Option<DispatcherQueueController>> = const { RefCell::new(None) };
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

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
struct GaussianBlurEffect {
    name: RefCell<HSTRING>,
    blur_amount: f32,
    source: IGraphicsEffectSource,
}

impl GaussianBlurEffect {
    fn new(blur_amount: f32) -> Result<Self> {
        let param = CompositionEffectSourceParameter::Create(&HSTRING::from("backdrop"))?;
        let source: IGraphicsEffectSource = param.cast()?;
        Ok(Self {
            name: RefCell::new(HSTRING::from("GaussianBlur")),
            blur_amount,
            source,
        })
    }
}

impl IGraphicsEffect_Impl for GaussianBlurEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.borrow().clone())
    }

    fn SetName(&self, name: &HSTRING) -> Result<()> {
        *self.name.borrow_mut() = name.clone();
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for GaussianBlurEffect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for GaussianBlurEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1_GAUSSIAN_BLUR)
    }

    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        unsafe {
            let name_str = name.to_string().unwrap_or_default();
            if name_str == "BlurAmount" {
                *index = 0;
                *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING(1); // GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT
                Ok(())
            } else if name_str == "BorderMode" {
                *index = 2;
                *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING(1);
                Ok(())
            } else {
                Err(windows::core::Error::from(E_INVALIDARG))
            }
        }
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        let insp: IInspectable = match index {
            0 => PropertyValue::CreateSingle(self.blur_amount)?,
            1 => PropertyValue::CreateUInt32(1)?, // D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED
            2 => PropertyValue::CreateUInt32(1)?, // D2D1_BORDER_MODE_HARD (1) — never 2!
            _ => return Err(windows::core::Error::from(E_INVALIDARG)),
        };
        insp.cast()
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0 {
            Ok(self.source.clone())
        } else {
            Err(windows::core::Error::from(E_INVALIDARG))
        }
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }
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

pub struct FrostedHost {
    compositor: Compositor,
    _target: DesktopWindowTarget,
    root_visual: ContainerVisual,
    backdrop_container: ContainerVisual,
    outer_clip_geom: CompositionRectangleGeometry,
    backdrop_visual: SpriteVisual,
    backdrop_clip_geom: CompositionRoundedRectangleGeometry,
    content_visual: SpriteVisual,
    _d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain1,
    host_brush_base: CompositionBrush,
    _effect_brush: CompositionEffectBrush,
    _surface_brush: CompositionSurfaceBrush,
    blur_amount: f32,
    width: u32,
    height: u32,
    bgra_scratch: Vec<u8>,
}

impl FrostedHost {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        ensure_dispatcher_queue()?;
        let w = width.max(1);
        let h = height.max(1);

        let compositor = Compositor::new()?;
        let desktop_interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { desktop_interop.CreateDesktopWindowTarget(hwnd, false)? };

        let root_visual = compositor.CreateContainerVisual()?;
        root_visual.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        let backdrop_container = compositor.CreateContainerVisual()?;
        backdrop_container.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        let outer_clip_geom = compositor.CreateRectangleGeometry()?;
        outer_clip_geom.SetOffset(Vector2 { X: 0.0, Y: 0.0 })?;
        outer_clip_geom.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        let outer_clip = compositor.CreateGeometricClipWithGeometry(&outer_clip_geom)?;
        backdrop_container.SetClip(&outer_clip)?;
        target.SetRoot(&root_visual)?;

        let host_brush = compositor.CreateHostBackdropBrush()?;
        let blur_effect: IGraphicsEffect = GaussianBlurEffect::new(20.0)?.into();
        let effect_factory = compositor.CreateEffectFactory(&blur_effect)?;
        let effect_brush = effect_factory.CreateBrush()?;
        let host_brush_base: CompositionBrush = host_brush.cast()?;
        effect_brush.SetSourceParameter(&HSTRING::from("backdrop"), &host_brush_base)?;

        let backdrop_visual = compositor.CreateSpriteVisual()?;
        backdrop_visual.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        let effect_brush_base: CompositionBrush = effect_brush.cast()?;
        backdrop_visual.SetBrush(&effect_brush_base)?;
        let backdrop_clip_geom = compositor.CreateRoundedRectangleGeometry()?;
        backdrop_clip_geom.SetOffset(Vector2 { X: 0.0, Y: 0.0 })?;
        backdrop_clip_geom.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        backdrop_clip_geom.SetCornerRadius(Vector2 { X: 22.0, Y: 22.0 })?;
        let backdrop_clip = compositor.CreateGeometricClipWithGeometry(&backdrop_clip_geom)?;
        backdrop_visual.SetClip(&backdrop_clip)?;
        backdrop_container.Children()?.InsertAtBottom(&backdrop_visual)?;
        root_visual.Children()?.InsertAtBottom(&backdrop_container)?;

        // D3D11 + DXGI SwapChainForComposition (PREMULTIPLIED RGBA->BGRA upload)
        let (d3d_device, d3d_context) = create_d3d11_device()?;
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
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
        let swap_chain = unsafe { factory.CreateSwapChainForComposition(&d3d_device, &desc, None)? };

        let comp_interop: ICompositorInterop = compositor.cast()?;
        let surface = unsafe { comp_interop.CreateCompositionSurfaceForSwapChain(&swap_chain)? };
        let surface_brush = compositor.CreateSurfaceBrushWithSurface(&surface)?;
        surface_brush.SetStretch(CompositionStretch::Fill)?;

        let content_visual = compositor.CreateSpriteVisual()?;
        content_visual.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        let surface_brush_base: CompositionBrush = surface_brush.cast()?;
        content_visual.SetBrush(&surface_brush_base)?;
        root_visual.Children()?.InsertAtTop(&content_visual)?;

        Ok(Self {
            compositor,
            _target: target,
            root_visual,
            backdrop_container,
            outer_clip_geom,
            backdrop_visual,
            backdrop_clip_geom,
            content_visual,
            _d3d_device: d3d_device,
            d3d_context,
            swap_chain,
            host_brush_base,
            _effect_brush: effect_brush,
            _surface_brush: surface_brush,
            blur_amount: 20.0,
            width: w,
            height: h,
            bgra_scratch: vec![0u8; (w as usize) * (h as usize) * 4],
        })
    }

    pub fn set_blur_amount(&mut self, px: f32) -> Result<()> {
        let clamped = px.clamp(0.0, 60.0);
        if (self.blur_amount - clamped).abs() < 0.1 {
            return Ok(());
        }
        let blur_effect: IGraphicsEffect = GaussianBlurEffect::new(clamped)?.into();
        let effect_factory = self.compositor.CreateEffectFactory(&blur_effect)?;
        let effect_brush = effect_factory.CreateBrush()?;
        effect_brush.SetSourceParameter(&HSTRING::from("backdrop"), &self.host_brush_base)?;
        let effect_brush_base: CompositionBrush = effect_brush.cast()?;
        self.backdrop_visual.SetBrush(&effect_brush_base)?;
        self._effect_brush = effect_brush;
        self.blur_amount = clamped;
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<()> {
        let w = width.max(1);
        let h = height.max(1);
        if self.width == w && self.height == h {
            return Ok(());
        }
        unsafe {
            self.swap_chain.ResizeBuffers(
                2,
                w,
                h,
                DXGI_FORMAT_B8G8R8A8_UNORM,
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        self.width = w;
        self.height = h;
        let sz = Vector2 {
            X: w as f32,
            Y: h as f32,
        };
        self.root_visual.SetSize(sz)?;
        self.backdrop_container.SetSize(sz)?;
        self.backdrop_visual.SetSize(sz)?;
        self.content_visual.SetSize(sz)?;
        let needed = (w as usize) * (h as usize) * 4;
        if self.bgra_scratch.len() != needed {
            self.bgra_scratch.resize(needed, 0);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn set_clip(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: (f32, f32, f32, f32),
        style: NotchStyle,
        placement: NotchPlacement,
    ) -> Result<()> {
        let (r_tl, r_tr, r_br, r_bl) = radii;
        let max_corner = (w.min(h) * 0.5).max(0.0);
        let r = r_tl.max(r_tr).max(r_br).max(r_bl).clamp(0.0, max_corner);
        self.outer_clip_geom.SetOffset(Vector2 {
            X: x.max(0.0),
            Y: y.max(0.0),
        })?;
        self.outer_clip_geom.SetSize(Vector2 {
            X: w.max(1.0),
            Y: h.max(1.0),
        })?;

        let ext_top = (r_tl < 1.0 && r_tr < 1.0)
            || (style == NotchStyle::EdgeFlush && placement == NotchPlacement::Top);
        let ext_left = (r_tl < 1.0 && r_bl < 1.0)
            || (style == NotchStyle::EdgeFlush && placement == NotchPlacement::Left);
        let ext_right = (r_tr < 1.0 && r_br < 1.0)
            || (style == NotchStyle::EdgeFlush && placement == NotchPlacement::Right);

        let mut bx = x;
        let mut by = y;
        let mut bw = w;
        let mut bh = h;
        if ext_top {
            by -= r;
            bh += r;
        } else {
            by += 0.5;
            bh = (bh - 0.5).max(1.0);
        }
        if ext_left {
            bx -= r;
            bw += r;
        } else {
            bx += 0.5;
            bw = (bw - 0.5).max(1.0);
        }
        if ext_right {
            bw += r;
        } else {
            bw = (bw - 0.5).max(1.0);
        }
        bh = (bh - 0.5).max(1.0);
        self.backdrop_clip_geom.SetOffset(Vector2 { X: bx, Y: by })?;
        self.backdrop_clip_geom.SetSize(Vector2 {
            X: bw.max(1.0),
            Y: bh.max(1.0),
        })?;
        self.backdrop_clip_geom
            .SetCornerRadius(Vector2 { X: r, Y: r })?;
        Ok(())
    }

    pub fn present_rgba_premultiplied(
        &mut self,
        rgba_data: &[u8],
        width: u32,
        height: u32,
    ) -> Result<()> {
        if width != self.width || height != self.height {
            self.resize(width, height)?;
        }
        let expected = (self.width as usize) * (self.height as usize) * 4;
        if rgba_data.len() < expected {
            return Err(windows::core::Error::from(E_INVALIDARG));
        }

        for (dst, src) in self.bgra_scratch[..expected]
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(rgba_data[..expected].as_chunks::<4>().0.iter())
        {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }

        unsafe {
            let back_buf: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
            let resource: ID3D11Resource = back_buf.cast()?;
            let row_pitch = self.width * 4;
            let depth_pitch = expected as u32;
            self.d3d_context.UpdateSubresource(
                &resource,
                0,
                None,
                self.bgra_scratch.as_ptr() as *const core::ffi::c_void,
                row_pitch,
                depth_pitch,
            );
            self.swap_chain.Present(0, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }
}
