#![allow(unsafe_op_in_unsafe_fn)]

use std::f32::consts::PI;
use std::sync::Mutex;
use windows::Foundation::{IPropertyValue, PropertyValue};
use windows::Graphics::Effects::{
    IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectSource_Impl, IGraphicsEffect_Impl,
};
use windows::Win32::Foundation::E_INVALIDARG;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_END_CLOSED, D2D_POINT_2F,
    D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    CLSID_D2D1ColorMatrix, CLSID_D2D1GaussianBlur, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE,
    D2D1_ARC_SIZE_LARGE, D2D1_ARC_SIZE_SMALL, D2D1_COLORMATRIX_PROP_ALPHA_MODE,
    D2D1_COLORMATRIX_PROP_CLAMP_OUTPUT, D2D1_COLORMATRIX_PROP_COLOR_MATRIX,
    D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED, D2D1_GAUSSIANBLUR_PROP_BORDER_MODE,
    D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION, D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION,
    D2D1_SWEEP_DIRECTION_CLOCKWISE, ID2D1Factory1, ID2D1PathGeometry,
};
use windows::Win32::System::WinRT::Graphics::Direct2D::{
    GRAPHICS_EFFECT_PROPERTY_MAPPING, GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT,
    IGraphicsEffectD2D1Interop, IGraphicsEffectD2D1Interop_Impl,
};
use windows::core::{GUID, HSTRING, Interface, PCWSTR, Result, implement};

/// Continuous blur specification for a single-HWND `DCompHost` backdrop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurSpec {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// Corner radii: `(top_left, top_right, bottom_right, bottom_left)`.
    pub radii: (f32, f32, f32, f32),
    /// User slider value in `4..=40`.
    pub blur_px: u32,
}

/// Fixed calibrated blur radius for v0.3.1 Obsidian Liquid Glass (15px airy liquid refraction).
pub const CALIBRATED_BLUR_PX: u32 = 15;

/// Maps `frosted_blur_px` (`4..=40`) smoothly and monotonically to a Direct2D
/// Gaussian blur standard deviation in DIPs (`1.5..=24.0`).
pub fn blur_px_to_sigma(px: u32) -> f32 {
    let p = px.clamp(4, 40) as f32;
    1.5 + (p - 4.0) * (22.5 / 36.0)
}

/// Option B: Optical Luminosity Clamp 5×4 Direct2D color matrix.
///
/// Combines Rec.709 saturation boost (`1.50`) with airy optical brightness preservation (`0.68`)
/// and a subtle smoked-obsidian floor `(0.004, 0.003, 0.010)` so bright backgrounds compress
/// comfortably while dark desktop apps and wallpapers remain visibly translucent and liquid
/// behind CyNotch — with `0%` CPU screen-capture overhead.
pub fn optical_luminosity_clamp_matrix() -> [f32; 20] {
    let b = 0.68_f32;
    let s = 1.50_f32;
    let inv_s = 1.0 - s;
    let lr = 0.2126_f32;
    let lg = 0.7152_f32;
    let lb = 0.0722_f32;

    let r0 = b * (lr * inv_s);
    let g0 = b * (lg * inv_s);
    let b0 = b * (lb * inv_s);
    let bs = b * s;

    // Direct2D D2D_MATRIX_5X4_F layout: [R' G' B' A'] = [R G B A 1] * M
    [
        r0 + bs, r0,      r0,      0.0, // Row 0: input R -> (R', G', B', A')
        g0,      g0 + bs, g0,      0.0, // Row 1: input G -> (R', G', B', A')
        b0,      b0,      b0 + bs, 0.0, // Row 2: input B -> (R', G', B', A')
        0.0,     0.0,     0.0,     1.0, // Row 3: input A -> (R', G', B', A')
        0.004,   0.003,   0.010,   0.0, // Row 4: subtle smoked obsidian floor offset
    ]
}

/// Evaluates a Direct2D 5×4 color matrix with output clamped to `[0.0, 1.0]`.
pub fn apply_color_matrix_5x4(rgb: [f32; 3], m: &[f32; 20]) -> [f32; 3] {
    let [r, g, b] = rgb;
    let out_r = (r * m[0] + g * m[4] + b * m[8] + m[16]).clamp(0.0, 1.0);
    let out_g = (r * m[1] + g * m[5] + b * m[9] + m[17]).clamp(0.0, 1.0);
    let out_b = (r * m[2] + g * m[6] + b * m[10] + m[18]).clamp(0.0, 1.0);
    [out_r, out_g, out_b]
}

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
pub struct ColorMatrixEffect {
    matrix: [f32; 20],
    source: Option<IGraphicsEffectSource>,
    name: Mutex<HSTRING>,
}

impl ColorMatrixEffect {
    pub fn new(matrix: [f32; 20], source: Option<IGraphicsEffectSource>) -> Self {
        Self {
            matrix,
            source,
            name: Mutex::new(HSTRING::from("LumClamp")),
        }
    }
}

impl IGraphicsEffect_Impl for ColorMatrixEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.lock().map(|g| g.clone()).unwrap_or_default())
    }

    fn SetName(&self, name: &HSTRING) -> Result<()> {
        if let Ok(mut g) = self.name.lock() {
            *g = name.clone();
        }
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for ColorMatrixEffect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for ColorMatrixEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1ColorMatrix)
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0
            && let Some(src) = &self.source
        {
            return Ok(src.clone());
        }
        Err(windows::core::Error::from(E_INVALIDARG))
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingleArray(&self.matrix)?.cast(),
            1 => PropertyValue::CreateUInt32(D2D1_ALPHA_MODE_PREMULTIPLIED.0 as u32)?.cast(),
            2 => PropertyValue::CreateBoolean(true)?.cast(),
            _ => Err(windows::core::Error::from(E_INVALIDARG)),
        }
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        if index.is_null() || mapping.is_null() {
            return Err(windows::core::Error::from(E_INVALIDARG));
        }
        let s = unsafe { name.to_string().unwrap_or_default() };
        match s.as_str() {
            "ColorMatrix" => {
                unsafe {
                    *index = D2D1_COLORMATRIX_PROP_COLOR_MATRIX.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
                Ok(())
            }
            "AlphaMode" => {
                unsafe {
                    *index = D2D1_COLORMATRIX_PROP_ALPHA_MODE.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
                Ok(())
            }
            "ClampOutput" => {
                unsafe {
                    *index = D2D1_COLORMATRIX_PROP_CLAMP_OUTPUT.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
                Ok(())
            }
            _ => Err(windows::core::Error::from(E_INVALIDARG)),
        }
    }
}

#[implement(IGraphicsEffect, IGraphicsEffectSource, IGraphicsEffectD2D1Interop)]
pub struct GaussianBlurEffect {
    sigma: f32,
    source: Option<IGraphicsEffectSource>,
    name: Mutex<HSTRING>,
}

impl GaussianBlurEffect {
    pub fn new(sigma: f32, source: Option<IGraphicsEffectSource>) -> Self {
        Self {
            sigma,
            source,
            name: Mutex::new(HSTRING::from("Blur")),
        }
    }
}

impl IGraphicsEffect_Impl for GaussianBlurEffect_Impl {
    fn Name(&self) -> Result<HSTRING> {
        Ok(self.name.lock().map(|g| g.clone()).unwrap_or_default())
    }

    fn SetName(&self, name: &HSTRING) -> Result<()> {
        if let Ok(mut g) = self.name.lock() {
            *g = name.clone();
        }
        Ok(())
    }
}

impl IGraphicsEffectSource_Impl for GaussianBlurEffect_Impl {}

impl IGraphicsEffectD2D1Interop_Impl for GaussianBlurEffect_Impl {
    fn GetEffectId(&self) -> Result<GUID> {
        Ok(CLSID_D2D1GaussianBlur)
    }

    fn GetSourceCount(&self) -> Result<u32> {
        Ok(1)
    }

    fn GetSource(&self, index: u32) -> Result<IGraphicsEffectSource> {
        if index == 0
            && let Some(src) = &self.source
        {
            return Ok(src.clone());
        }
        Err(windows::core::Error::from(E_INVALIDARG))
    }

    fn GetPropertyCount(&self) -> Result<u32> {
        Ok(3)
    }

    fn GetProperty(&self, index: u32) -> Result<IPropertyValue> {
        match index {
            0 => PropertyValue::CreateSingle(self.sigma)?.cast(),
            1 => PropertyValue::CreateUInt32(D2D1_GAUSSIANBLUR_OPTIMIZATION_BALANCED.0 as u32)?
                .cast(),
            2 => PropertyValue::CreateUInt32(1)?.cast(), // D2D1_BORDER_MODE_HARD = 1
            _ => Err(windows::core::Error::from(E_INVALIDARG)),
        }
    }

    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    fn GetNamedPropertyMapping(
        &self,
        name: &PCWSTR,
        index: *mut u32,
        mapping: *mut GRAPHICS_EFFECT_PROPERTY_MAPPING,
    ) -> Result<()> {
        if index.is_null() || mapping.is_null() {
            return Err(windows::core::Error::from(E_INVALIDARG));
        }
        let s = unsafe { name.to_string().unwrap_or_default() };
        match s.as_str() {
            "BlurAmount" => {
                unsafe {
                    *index = D2D1_GAUSSIANBLUR_PROP_STANDARD_DEVIATION.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
                Ok(())
            }
            "Optimization" => {
                unsafe {
                    *index = D2D1_GAUSSIANBLUR_PROP_OPTIMIZATION.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
                Ok(())
            }
            "BorderMode" => {
                unsafe {
                    *index = D2D1_GAUSSIANBLUR_PROP_BORDER_MODE.0 as u32;
                    *mapping = GRAPHICS_EFFECT_PROPERTY_MAPPING_DIRECT;
                }
                Ok(())
            }
            _ => Err(windows::core::Error::from(E_INVALIDARG)),
        }
    }
}

fn arc_endpoints(
    cx: f32,
    cy: f32,
    r: f32,
    a0: f32,
    sweep: f32,
) -> (D2D_POINT_2F, D2D_POINT_2F, D2D1_ARC_SIZE) {
    let start_pt = D2D_POINT_2F {
        x: cx + r * a0.cos(),
        y: cy + r * a0.sin(),
    };
    let a1 = a0 + sweep;
    let end_pt = D2D_POINT_2F {
        x: cx + r * a1.cos(),
        y: cy + r * a1.sin(),
    };
    let arc_size = if sweep.abs() > PI {
        D2D1_ARC_SIZE_LARGE
    } else {
        D2D1_ARC_SIZE_SMALL
    };
    (start_pt, end_pt, arc_size)
}

/// Builds a 4-corner-rounded Direct2D path matching `PainterD2D::rrect_corners4_vertical_gradient` 1:1.
///
/// # Safety
/// `factory` must be a valid Direct2D factory bound to the caller's thread/device.
pub unsafe fn build_d2d_rounded_path(
    factory: &ID2D1Factory1,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radii: (f32, f32, f32, f32),
) -> Result<ID2D1PathGeometry> {
    let max_r = (w * 0.5).min(h * 0.5).max(0.0);
    let rt = radii.0.clamp(0.0, max_r);
    let rtr = radii.1.clamp(0.0, max_r);
    let rbr = radii.2.clamp(0.0, max_r);
    let rbl = radii.3.clamp(0.0, max_r);

    let path = factory.CreatePathGeometry()?;
    let sink = path.Open()?;

    sink.BeginFigure(D2D_POINT_2F { x: x + rt, y }, D2D1_FIGURE_BEGIN_FILLED);
    sink.AddLine(D2D_POINT_2F { x: x + w - rtr, y });
    if rtr > 0.0 {
        let (_, end_pt, arc_size) = arc_endpoints(x + w - rtr, y + rtr, rtr, -PI / 2.0, PI / 2.0);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: end_pt,
            size: D2D_SIZE_F {
                width: rtr,
                height: rtr,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: arc_size,
        });
    }
    sink.AddLine(D2D_POINT_2F {
        x: x + w,
        y: y + h - rbr,
    });
    if rbr > 0.0 {
        let (_, end_pt, arc_size) = arc_endpoints(x + w - rbr, y + h - rbr, rbr, 0.0, PI / 2.0);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: end_pt,
            size: D2D_SIZE_F {
                width: rbr,
                height: rbr,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: arc_size,
        });
    }
    sink.AddLine(D2D_POINT_2F {
        x: x + rbl,
        y: y + h,
    });
    if rbl > 0.0 {
        let (_, end_pt, arc_size) = arc_endpoints(x + rbl, y + h - rbl, rbl, PI / 2.0, PI / 2.0);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: end_pt,
            size: D2D_SIZE_F {
                width: rbl,
                height: rbl,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: arc_size,
        });
    }
    sink.AddLine(D2D_POINT_2F { x, y: y + rt });
    if rt > 0.0 {
        let (_, end_pt, arc_size) = arc_endpoints(x + rt, y + rt, rt, PI, PI / 2.0);
        sink.AddArc(&D2D1_ARC_SEGMENT {
            point: end_pt,
            size: D2D_SIZE_F {
                width: rt,
                height: rt,
            },
            rotationAngle: 0.0,
            sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
            arcSize: arc_size,
        });
    }
    sink.EndFigure(D2D1_FIGURE_END_CLOSED);
    sink.Close()?;
    Ok(path.into())
}
