#[cfg(target_os = "windows")]
use windows::{
    core::Interface,
    Win32::Foundation::*,
    Win32::Graphics::Direct3D::*,
    Win32::Graphics::Direct3D11::*,
    Win32::Graphics::Dxgi::Common::*,
    Win32::Graphics::Dxgi::*,
    Win32::System::Com::*,
};
use crate::capture::{Frame, FrameSource};
use std::sync::Arc;

#[cfg(target_os = "windows")]
fn create_black_bgra(width: u32, height: u32) -> Arc<Vec<u8>> {
    let mut buf = vec![0u8; (width * 4 * height) as usize];
    for chunk in buf.chunks_exact_mut(4) {
        chunk[0] = 0;   // B
        chunk[1] = 0;   // G
        chunk[2] = 0;   // R
        chunk[3] = 255; // A (opaque black)
    }
    Arc::new(buf)
}

#[cfg(target_os = "windows")]
pub struct WindowsCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: Option<IDXGIOutputDuplication>,
    staging_texture: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    pub last_stride: u32,
    buffer_pool: Vec<Option<Arc<Vec<u8>>>>,
    last_frame: Option<Arc<Vec<u8>>>,
}

#[cfg(target_os = "windows")]
unsafe impl Send for WindowsCapture {}

#[cfg(target_os = "windows")]
impl WindowsCapture {
    pub fn new() -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        unsafe {
            // Initialize COM library on capture thread
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            // Create DXGI Factory to find the display adapter with active outputs
            let factory: IDXGIFactory1 = CreateDXGIFactory1()?;
            let mut successful_session: Option<(ID3D11Device, ID3D11DeviceContext, IDXGIOutputDuplication, u32, u32)> = None;

            let mut a_idx = 0;
            'adapter_loop: while let Ok(adapter) = factory.EnumAdapters1(a_idx) {
                let mut o_idx = 0;
                while let Ok(output) = adapter.EnumOutputs(o_idx) {
                    if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                        if let Ok(desc) = output1.GetDesc() {
                            let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).unsigned_abs();
                            let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).unsigned_abs();
                            if width > 0 && height > 0 {
                                if let Ok(adapter_base) = adapter.cast::<IDXGIAdapter>() {
                                    let mut device: Option<ID3D11Device> = None;
                                    let mut context: Option<ID3D11DeviceContext> = None;
                                    let mut feature_level = D3D_FEATURE_LEVEL_11_0;

                                    let create_res = D3D11CreateDevice(
                                        Some(&adapter_base),
                                        D3D_DRIVER_TYPE_UNKNOWN,
                                        HMODULE(std::ptr::null_mut()),
                                        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                        Some(&[
                                            D3D_FEATURE_LEVEL_11_1,
                                            D3D_FEATURE_LEVEL_11_0,
                                            D3D_FEATURE_LEVEL_10_1,
                                            D3D_FEATURE_LEVEL_10_0,
                                        ]),
                                        D3D11_SDK_VERSION,
                                        Some(&mut device),
                                        Some(&mut feature_level),
                                        Some(&mut context),
                                    );

                                    if create_res.is_ok() {
                                        if let (Some(dev), Some(ctx)) = (device, context) {
                                            match output1.DuplicateOutput(&dev) {
                                                Ok(dup) => {
                                                    successful_session = Some((dev, ctx, dup, width, height));
                                                    break 'adapter_loop;
                                                }
                                                Err(e) => {
                                                    eprintln!("Adapter {} Output {} DuplicateOutput failed: {:?}", a_idx, o_idx, e);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    o_idx += 1;
                }
                a_idx += 1;
            }

            let (device, context, duplication, width, height) = successful_session
                .ok_or("Failed to initialize Windows Desktop Duplication on any display output")?;

            let staging_desc = D3D11_TEXTURE2D_DESC {
                Width: width,
                Height: height,
                MipLevels: 1,
                ArraySize: 1,
                Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                MiscFlags: 0,
            };

            let mut staging_texture_opt: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging_texture_opt))?;
            let staging_texture = staging_texture_opt.ok_or("Failed to create D3D11 staging texture")?;

            println!("Windows DXGI Hardware Desktop Duplication active: {}x{}", width, height);

            Ok(Self {
                device,
                context,
                duplication: Some(duplication),
                staging_texture,
                width,
                height,
                last_stride: width * 4,
                buffer_pool: Vec::with_capacity(4),
                last_frame: None,
            })
        }
    }

    fn reinit_duplication(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.duplication = None;
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Ok(new_cap) = WindowsCapture::new() {
            self.device = new_cap.device;
            self.context = new_cap.context;
            self.duplication = new_cap.duplication;
            self.staging_texture = new_cap.staging_texture;
            self.width = new_cap.width;
            self.height = new_cap.height;
            self.last_stride = new_cap.last_stride;
            self.buffer_pool.clear();
            return Ok(());
        }
        Err("Failed to reinitialize desktop duplication".into())
    }
}

#[cfg(target_os = "windows")]
impl FrameSource for WindowsCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        unsafe {
            if self.duplication.is_none() {
                let _ = self.reinit_duplication();
            }

            let mut dup = match self.duplication.clone() {
                Some(d) => d,
                None => {
                    return Ok(Frame::Raw {
                        width: self.width,
                        height: self.height,
                        stride: self.last_stride,
                        data: self.last_frame.clone().unwrap_or_else(|| create_black_bgra(self.width, self.height)),
                    });
                }
            };

            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut desktop_resource: Option<IDXGIResource> = None;

            for _ in 0..4 {
                match dup.AcquireNextFrame(8, &mut frame_info, &mut desktop_resource) {
                    Ok(()) => {
                        if let Some(resource) = desktop_resource {
                            let texture: ID3D11Texture2D = resource.cast()?;
                            
                            // Copy GPU desktop texture into CPU staging texture
                            self.context.CopyResource(&self.staging_texture, &texture);
                            let _ = dup.ReleaseFrame();

                            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                            self.context.Map(&self.staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

                            let target_stride = (self.width * 4) as usize;
                            let total_bytes = target_stride * self.height as usize;

                            // Acquire buffer from pool
                            let mut arc_buf = None;
                            for slot in &mut self.buffer_pool {
                                if let Some(buf) = slot {
                                    if Arc::strong_count(buf) == 1 {
                                        arc_buf = slot.take();
                                        break;
                                    }
                                }
                            }
                            let mut arc_buf = arc_buf.unwrap_or_else(|| Arc::new(vec![0u8; total_bytes]));
                            {
                                let vec_ref = Arc::get_mut(&mut arc_buf).unwrap();
                                if vec_ref.len() != total_bytes {
                                    vec_ref.resize(total_bytes, 0);
                                }
                                
                                let src_pitch = mapped.RowPitch as usize;
                                if src_pitch == target_stride {
                                    std::ptr::copy_nonoverlapping(
                                        mapped.pData as *const u8,
                                        vec_ref.as_mut_ptr(),
                                        total_bytes,
                                    );
                                } else {
                                    for y in 0..self.height as usize {
                                        let src_row = (mapped.pData as *const u8).add(y * src_pitch);
                                        let dst_row = vec_ref.as_mut_ptr().add(y * target_stride);
                                        std::ptr::copy_nonoverlapping(src_row, dst_row, target_stride);
                                    }
                                }
                            }
                            self.context.Unmap(&self.staging_texture, 0);

                            let to_send = Arc::clone(&arc_buf);
                            self.last_frame = Some(Arc::clone(&arc_buf));
                            self.last_stride = target_stride as u32;

                            if self.buffer_pool.len() < 4 {
                                self.buffer_pool.push(Some(arc_buf));
                            }

                            return Ok(Frame::Raw {
                                width: self.width,
                                height: self.height,
                                stride: self.last_stride,
                                data: to_send,
                            });
                        }
                        let _ = dup.ReleaseFrame();
                    }
                    Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                        // Desktop static / no new frame presented
                        break;
                    }
                    Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST
                        || e.code() == DXGI_ERROR_ACCESS_DENIED
                        || e.code() == DXGI_ERROR_INVALID_CALL => {
                        let _ = dup.ReleaseFrame();
                        drop(dup);
                        self.duplication = None;
                        let _ = self.reinit_duplication();
                        break;
                    }
                    Err(e) => {
                        let _ = dup.ReleaseFrame();
                        drop(dup);
                        self.duplication = None;
                        let _ = self.reinit_duplication();
                        return Err(Box::new(e));
                    }
                }
            }

            // Return last frame or black frame if idle
            Ok(Frame::Raw {
                width: self.width,
                height: self.height,
                stride: self.last_stride,
                data: self.last_frame.clone().unwrap_or_else(|| create_black_bgra(self.width, self.height)),
            })
        }
    }
}
