#![allow(unsafe_op_in_unsafe_fn)]

use std::cell::RefCell;
use windows::Foundation::Numerics::Vector2;
use windows::System::DispatcherQueueController;
use windows::UI::Composition::Desktop::DesktopWindowTarget;
use windows::UI::Composition::{
    CompositionBrush, CompositionStretch, CompositionSurfaceBrush, Compositor, ContainerVisual,
    SpriteVisual,
};
use windows::Win32::Foundation::{E_INVALIDARG, FALSE, HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::{
    D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP, D3D_FEATURE_LEVEL_10_0, D3D_FEATURE_LEVEL_11_0,
    D3D_FEATURE_LEVEL_11_1,
};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BOX, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION, D3D11CreateDevice,
    ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    DXGI_PRESENT, DXGI_SCALING_STRETCH, DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG,
    DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT, IDXGIAdapter, IDXGIDevice,
    IDXGIFactory2, IDXGISwapChain1,
};
use windows::Win32::System::WinRT::Composition::{ICompositorDesktopInterop, ICompositorInterop};
use windows::Win32::System::WinRT::{
    CreateDispatcherQueueController, DQTAT_COM_NONE, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
};
use windows::core::{Interface, Result};

#[derive(Clone)]
struct SharedDComp {
    compositor: Compositor,
    d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    dxgi_factory: IDXGIFactory2,
}

thread_local! {
    static DISPATCHER_CONTROLLER: RefCell<Option<DispatcherQueueController>> = const { RefCell::new(None) };
    static SHARED_DCOMP: RefCell<Option<SharedDComp>> = const { RefCell::new(None) };
}

fn shared_dcomp() -> Result<SharedDComp> {
    SHARED_DCOMP.with_borrow_mut(|slot| {
        if let Some(shared) = slot.as_ref() {
            return Ok(shared.clone());
        }
        let compositor = Compositor::new()?;
        let (d3d_device, d3d_context) = create_d3d11_device()?;
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let adapter: IDXGIAdapter = unsafe { dxgi_device.GetAdapter()? };
        let dxgi_factory: IDXGIFactory2 = unsafe { adapter.GetParent()? };
        let shared = SharedDComp {
            compositor,
            d3d_device,
            d3d_context,
            dxgi_factory,
        };
        *slot = Some(shared.clone());
        Ok(shared)
    })
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
pub struct DCompHost {
    _compositor: Compositor,
    _target: DesktopWindowTarget,
    root_visual: ContainerVisual,
    content_visual: SpriteVisual,
    _d3d_device: ID3D11Device,
    d3d_context: ID3D11DeviceContext,
    swap_chain: IDXGISwapChain1,
    _surface_brush: CompositionSurfaceBrush,
    width: u32,
    height: u32,
    bgra_scratch: Vec<u8>,
    prev_dirty: Option<(u32, u32, u32, u32)>,
}

impl DCompHost {
    pub fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self> {
        ensure_dispatcher_queue()?;
        let w = width.max(1);
        let h = height.max(1);

        let SharedDComp {
            compositor,
            d3d_device,
            d3d_context,
            dxgi_factory: factory,
        } = shared_dcomp()?;
        let desktop_interop: ICompositorDesktopInterop = compositor.cast()?;
        let target = unsafe { desktop_interop.CreateDesktopWindowTarget(hwnd, false)? };

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

        let content_visual = compositor.CreateSpriteVisual()?;
        content_visual.SetSize(Vector2 {
            X: w as f32,
            Y: h as f32,
        })?;
        let surface_brush_base: CompositionBrush = surface_brush.cast()?;
        content_visual.SetBrush(&surface_brush_base)?;
        root_visual.Children()?.InsertAtTop(&content_visual)?;

        Ok(Self {
            _compositor: compositor,
            _target: target,
            root_visual,
            content_visual,
            _d3d_device: d3d_device,
            d3d_context,
            swap_chain,
            _surface_brush: surface_brush,
            width: w,
            height: h,
            bgra_scratch: vec![0u8; (w as usize) * (h as usize) * 4],
            prev_dirty: None,
        })
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
        self.content_visual.SetSize(sz)?;
        let needed = (w as usize) * (h as usize) * 4;
        if self.bgra_scratch.len() != needed {
            self.bgra_scratch.resize(needed, 0);
            if needed < self.bgra_scratch.capacity() {
                self.bgra_scratch.shrink_to_fit();
            }
        }
        self.prev_dirty = None;
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

        unsafe {
            let back_buf: ID3D11Texture2D = self.swap_chain.GetBuffer(0)?;
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
            self.swap_chain.Present(0, DXGI_PRESENT(0)).ok()?;
        }
        Ok(())
    }
}

fn swizzle_rgba_to_bgra(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.as_chunks::<4>().0.iter())
    {
        let px = u32::from_le_bytes(*s);
        *d = ((px & 0xFF00_FF00) | ((px >> 16) & 0xFF) | ((px & 0xFF) << 16)).to_le_bytes();
    }
}
