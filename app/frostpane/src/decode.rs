//! Image decoder for the wallpaper thumbnails (`HostInfo::decoder`): Windows
//! Imaging Component, scaled to a maximum side, straight-alpha RGBA.

use std::path::Path;

/// Size that fits `max_side` on the longer side, never enlarging, never 0.
pub fn fit_size(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let longest = w.max(h);
    if longest == 0 || max_side == 0 || longest <= max_side {
        return (w.max(1), h.max(1));
    }
    let scale = f64::from(max_side) / f64::from(longest);
    (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1))
}

/// Decode `path`; `None` when the file cannot be read as an image.
#[cfg(windows)]
pub fn decode(path: &Path, max_side: u32) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::*;
    use windows::core::HSTRING;

    thread_local! {
        // COM is per thread: initialise it once on every thread that decodes.
        static COM: () = {
            // SAFETY: plain COM init; a different apartment mode already set
            // by the window toolkit only returns an error code, which is fine.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        };
    }
    COM.with(|_| {});
    // SAFETY: WIC calls on valid interfaces created here; the pixel buffer
    // is sized for the stride and height passed to CopyPixels.
    unsafe {
        let factory: IWICImagingFactory =
            CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory
            .CreateDecoderFromFilename(
                &HSTRING::from(path.as_os_str()),
                None,
                GENERIC_READ,
                WICDecodeMetadataCacheOnDemand,
            )
            .ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        let (mut w, mut h) = (0u32, 0u32);
        frame.GetSize(&mut w, &mut h).ok()?;
        let (nw, nh) = fit_size(w, h, max_side);
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&frame, nw, nh, WICBitmapInterpolationModeFant).ok()?;
        let convert = factory.CreateFormatConverter().ok()?;
        convert
            .Initialize(
                &scaler,
                &GUID_WICPixelFormat32bppRGBA,
                WICBitmapDitherTypeNone,
                None,
                0.0,
                WICBitmapPaletteTypeCustom,
            )
            .ok()?;
        let mut rgba = vec![0u8; nw as usize * nh as usize * 4];
        convert.CopyPixels(std::ptr::null(), nw * 4, &mut rgba).ok()?;
        Some((nw, nh, rgba))
    }
}

/// No decoder off Windows: the tiles use their drawn pictures.
#[cfg(not(windows))]
pub fn decode(_path: &Path, _max_side: u32) -> Option<(u32, u32, Vec<u8>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(fit_size(100, 50, 200), (100, 50));
        assert_eq!(fit_size(1000, 500, 100), (100, 50));
        assert_eq!(fit_size(500, 1000, 100), (50, 100));
        assert_eq!(fit_size(1000, 1, 10), (10, 1));
        assert_eq!(fit_size(0, 0, 10), (1, 1));
        assert_eq!(fit_size(10, 10, 0), (10, 10));
    }

    /// 2x2 24-bit BMP: bottom row red, green; top row blue, white.
    fn bmp() -> Vec<u8> {
        let mut v = b"BM".to_vec();
        let px: [[u8; 3]; 4] = [[0, 0, 255], [0, 255, 0], [255, 0, 0], [255, 255, 255]]; // BGR, bottom-up
        let mut data = Vec::new();
        for row in px.chunks(2) {
            for p in row {
                data.extend_from_slice(p);
            }
            data.extend_from_slice(&[0, 0]); // row padding to 4 bytes
        }
        v.extend_from_slice(&(54 + data.len() as u32).to_le_bytes());
        v.extend_from_slice(&[0; 4]);
        v.extend_from_slice(&54u32.to_le_bytes());
        v.extend_from_slice(&40u32.to_le_bytes());
        v.extend_from_slice(&2i32.to_le_bytes());
        v.extend_from_slice(&2i32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&24u16.to_le_bytes());
        v.extend_from_slice(&[0; 24]);
        v.extend_from_slice(&data);
        v
    }

    #[cfg(windows)]
    #[test]
    fn decodes_a_bitmap_to_rgba() {
        let path = std::env::temp_dir().join(format!("fp-decode-{}.bmp", std::process::id()));
        std::fs::write(&path, bmp()).unwrap();
        let (w, h, rgba) = decode(&path, 64).expect("decoded");
        assert_eq!((w, h), (2, 2));
        assert_eq!(rgba.len(), 16);
        // Top-left is the blue pixel, straight alpha 255.
        assert_eq!(&rgba[0..4], &[0, 0, 255, 255]);
        assert_eq!(&rgba[12..16], &[0, 255, 0, 255]);
        let (w, h, _) = decode(&path, 1).expect("scaled");
        assert_eq!((w, h), (1, 1));
        std::fs::write(&path, b"not an image").unwrap();
        assert!(decode(&path, 64).is_none());
        let _ = std::fs::remove_file(&path);
        assert!(decode(&path, 64).is_none());
    }

    /// COM is set up per thread, so other threads decode too.
    #[cfg(windows)]
    #[test]
    fn decodes_on_any_thread() {
        let path = std::env::temp_dir().join(format!("fp-decode-thread-{}.bmp", std::process::id()));
        std::fs::write(&path, bmp()).unwrap();
        let run = |p: std::path::PathBuf| std::thread::spawn(move || decode(&p, 64).map(|d| (d.0, d.1)));
        let (a, b) = (run(path.clone()), run(path.clone()));
        let (a, b) = (a.join().unwrap(), b.join().unwrap());
        let _ = std::fs::remove_file(&path);
        assert_eq!(a, Some((2, 2)));
        assert_eq!(b, Some((2, 2)));
    }
}
