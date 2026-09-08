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
pub struct WindowsCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging_textures: [ID3D11Texture2D; 2],
    staging_idx: usize,
    has_staged_frame: bool,
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
            let mut chosen_adapter: Option<IDXGIAdapter1> = None;
            let mut chosen_output: Option<IDXGIOutput1> = None;

            let mut a_idx = 0;
            while let Ok(adapter) = factory.EnumAdapters1(a_idx) {
                let mut o_idx = 0;
                while let Ok(output) = adapter.EnumOutputs(o_idx) {
                    if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                        chosen_adapter = Some(adapter);
                        chosen_output = Some(output1);
                        break;
                    }
                    o_idx += 1;
                }
                if chosen_adapter.is_some() {
                    break;
                }
                a_idx += 1;
            }

            let (adapter, output1) = match (chosen_adapter, chosen_output) {
                (Some(a), Some(o)) => (a, o),
                _ => return Err("No active display output found for Windows desktop capture".into()),
            };

            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut feature_level = D3D_FEATURE_LEVEL_11_0;

            let adapter_base = adapter.cast::<IDXGIAdapter>()?;

            D3D11CreateDevice(
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
            )?;

            let device = device.ok_or("Failed to create D3D11 device")?;
            let context = context.ok_or("Failed to create D3D11 context")?;

            let desc = output1.GetDesc()?;
            let width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).unsigned_abs();
            let height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).unsigned_abs();

            // Initialize Desktop Duplication
            let duplication = output1.DuplicateOutput(&device)?;

            // Allocate double-buffered CPU-accessible staging textures for asynchronous zero-stall GPU readback
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

            let mut tex0: Option<ID3D11Texture2D> = None;
            let mut tex1: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut tex0))?;
            device.CreateTexture2D(&staging_desc, None, Some(&mut tex1))?;
            let staging_textures = [
                tex0.ok_or("Failed to create D3D11 staging texture 0")?,
                tex1.ok_or("Failed to create D3D11 staging texture 1")?,
            ];

            println!("Windows DXGI Hardware Desktop Duplication active (asynchronous double-buffered): {}x{}", width, height);

            Ok(Self {
                device,
                context,
                duplication,
                staging_textures,
                staging_idx: 0,
                has_staged_frame: false,
                width,
                height,
                last_stride: width * 4,
                buffer_pool: Vec::with_capacity(4),
                last_frame: None,
            })
        }
    }

    fn reinit_duplication(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        unsafe {
            let dxgi_device: IDXGIDevice = self.device.cast()?;
            let adapter = dxgi_device.GetAdapter()?;
            let mut o_idx = 0;
            while let Ok(output) = adapter.EnumOutputs(o_idx) {
                if let Ok(output1) = output.cast::<IDXGIOutput1>() {
                    if let Ok(dup) = output1.DuplicateOutput(&self.device) {
                        self.duplication = dup;
                        self.has_staged_frame = false;
                        self.staging_idx = 0;
                        if let Ok(desc) = output1.GetDesc() {
                            let new_w = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left).unsigned_abs();
                            let new_h = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top).unsigned_abs();
                            if new_w != self.width || new_h != self.height {
                                self.width = new_w;
                                self.height = new_h;
                                self.last_stride = new_w * 4;
                                self.buffer_pool.clear();
                                self.last_frame = None;
                                let staging_desc = D3D11_TEXTURE2D_DESC {
                                    Width: new_w,
                                    Height: new_h,
                                    MipLevels: 1,
                                    ArraySize: 1,
                                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                                    SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
                                    Usage: D3D11_USAGE_STAGING,
                                    BindFlags: 0,
                                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                                    MiscFlags: 0,
                                };
                                let mut tex0: Option<ID3D11Texture2D> = None;
                                let mut tex1: Option<ID3D11Texture2D> = None;
                                let ok0 = self.device.CreateTexture2D(&staging_desc, None, Some(&mut tex0)).is_ok();
                                let ok1 = self.device.CreateTexture2D(&staging_desc, None, Some(&mut tex1)).is_ok();
                                if ok0 && ok1 {
                                    if let (Some(t0), Some(t1)) = (tex0, tex1) {
                                        self.staging_textures = [t0, t1];
                                    }
                                }
                            }
                        }
                        return Ok(());
                    }
                }
                o_idx += 1;
            }
            Err("Failed to reinitialize desktop duplication".into())
        }
    }
}

#[cfg(target_os = "windows")]
impl FrameSource for WindowsCapture {
    fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        unsafe {
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut desktop_resource: Option<IDXGIResource> = None;

            for _ in 0..10 {
                match self.duplication.AcquireNextFrame(25, &mut frame_info, &mut desktop_resource) {
                    Ok(()) => {
                        if let Some(resource) = desktop_resource {
                            let texture: ID3D11Texture2D = resource.cast()?;
                            
                            let write_idx = self.staging_idx;
                            let read_idx = 1 - self.staging_idx;

                            // Asynchronously copy GPU desktop texture into staging texture[write_idx]
                            self.context.CopyResource(&self.staging_textures[write_idx], &texture);
                            
                            // Immediately release the desktop frame back to the DWM compositor
                            let _ = self.duplication.ReleaseFrame();

                            // Use ping-pong texture:
                            // On first frame, read write_idx directly.
                            // On subsequent frames, read read_idx (queued 1 frame ago, DMA already finished!)
                            let target_read = if !self.has_staged_frame {
                                self.has_staged_frame = true;
                                write_idx
                            } else {
                                read_idx
                            };
                            self.staging_idx = 1 - self.staging_idx;

                            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                            self.context.Map(&self.staging_textures[target_read], 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

                            let stride = mapped.RowPitch as usize;
                            self.last_stride = mapped.RowPitch;
                            let total_bytes = stride * self.height as usize;

                            // Reusable zero-allocation buffer acquisition from pool
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
                                std::ptr::copy_nonoverlapping(
                                    mapped.pData as *const u8,
                                    vec_ref.as_mut_ptr(),
                                    total_bytes,
                                );
                            }
                            self.context.Unmap(&self.staging_textures[target_read], 0);

                            let to_send = Arc::clone(&arc_buf);
                            self.last_frame = Some(Arc::clone(&arc_buf));

                            // Return to buffer pool
                            if self.buffer_pool.len() < 4 {
                                self.buffer_pool.push(Some(arc_buf));
                            }

                            return Ok(Frame::Raw {
                                width: self.width,
                                height: self.height,
                                stride: stride as u32,
                                data: to_send,
                            });
                        }
                        let _ = self.duplication.ReleaseFrame();
                    }
                    Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                        // Desktop static / no new frame presented. Sleep briefly and wait.
                        // Do NOT allocate duplicate frames!
                        std::thread::sleep(std::time::Duration::from_millis(3));
                        continue;
                    }
                    Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST
                        || e.code() == DXGI_ERROR_ACCESS_DENIED
                        || e.code() == DXGI_ERROR_INVALID_CALL => {
                        let _ = self.duplication.ReleaseFrame();
                        let _ = self.reinit_duplication();
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    Err(e) => {
                        let _ = self.duplication.ReleaseFrame();
                        let _ = self.reinit_duplication();
                        std::thread::sleep(std::time::Duration::from_millis(16));
                        return Err(Box::new(e));
                    }
                }
            }

            // If desktop was completely idle across multiple iterations,
            // return zero-copy reference to last frame so stream stays active without heap thrashing
            if let Some(ref last) = self.last_frame {
                Ok(Frame::Raw {
                    width: self.width,
                    height: self.height,
                    stride: self.last_stride,
                    data: Arc::clone(last),
                })
            } else {
                Ok(Frame::Raw {
                    width: self.width,
                    height: self.height,
                    stride: self.last_stride,
                    data: Arc::new(vec![0u8; (self.last_stride * self.height) as usize]),
                })
            }
        }
    }
}
