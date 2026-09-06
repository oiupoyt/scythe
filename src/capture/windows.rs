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

#[cfg(target_os = "windows")]
pub struct WindowsCapture {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    duplication: IDXGIOutputDuplication,
    staging_texture: ID3D11Texture2D,
    pub width: u32,
    pub height: u32,
    last_frame: Vec<u8>,
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
                Some(&[D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0]),
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

            // Allocate a CPU-accessible staging texture for hardware frame reading
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

            let mut staging_texture: Option<ID3D11Texture2D> = None;
            device.CreateTexture2D(&staging_desc, None, Some(&mut staging_texture))?;
            let staging_texture = staging_texture.ok_or("Failed to create D3D11 staging texture")?;

            println!("Windows DXGI Hardware Desktop Duplication active: {}x{}", width, height);

            Ok(Self {
                device,
                context,
                duplication,
                staging_texture,
                width,
                height,
                last_frame: Vec::new(),
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

            for _ in 0..5 {
                match self.duplication.AcquireNextFrame(100, &mut frame_info, &mut desktop_resource) {
                    Ok(()) => {
                        if let Some(resource) = desktop_resource {
                            let texture: ID3D11Texture2D = resource.cast()?;
                            
                            // Copy GPU desktop texture to staging texture accessible by CPU
                            self.context.CopyResource(&self.staging_texture, &texture);
                            
                            // Immediately release the desktop frame back to the DWM compositor
                            let _ = self.duplication.ReleaseFrame();

                            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
                            self.context.Map(&self.staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;

                            let stride = mapped.RowPitch as usize;
                            let total_bytes = stride * self.height as usize;
                            if self.last_frame.len() != total_bytes {
                                self.last_frame.resize(total_bytes, 0);
                            }
                            std::ptr::copy_nonoverlapping(
                                mapped.pData as *const u8,
                                self.last_frame.as_mut_ptr(),
                                total_bytes,
                            );
                            self.context.Unmap(&self.staging_texture, 0);

                            return Ok(Frame::Raw {
                                width: self.width,
                                height: self.height,
                                stride: stride as u32,
                                data: self.last_frame.clone(),
                            });
                        }
                        let _ = self.duplication.ReleaseFrame();
                    }
                    Err(e) if e.code() == DXGI_ERROR_WAIT_TIMEOUT => {
                        if !self.last_frame.is_empty() {
                            return Ok(Frame::Raw {
                                width: self.width,
                                height: self.height,
                                stride: self.width * 4,
                                data: self.last_frame.clone(),
                            });
                        }
                        std::thread::sleep(std::time::Duration::from_millis(8));
                    }
                    Err(e) if e.code() == DXGI_ERROR_ACCESS_LOST => {
                        println!("DXGI access lost (display mode or fullscreen switch), reacquiring...");
                        let _ = self.reinit_duplication();
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        continue;
                    }
                    Err(e) => {
                        return Err(Box::new(e));
                    }
                }
            }

            if !self.last_frame.is_empty() {
                Ok(Frame::Raw {
                    width: self.width,
                    height: self.height,
                    stride: self.width * 4,
                    data: self.last_frame.clone(),
                })
            } else {
                Ok(Frame::Raw {
                    width: self.width,
                    height: self.height,
                    stride: self.width * 4,
                    data: vec![0u8; (self.width * self.height * 4) as usize],
                })
            }
        }
    }
}
