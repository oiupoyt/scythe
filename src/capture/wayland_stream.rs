use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;
use std::os::unix::io::OwnedFd;
use crate::capture::Frame;
use crossbeam_channel::{bounded, Sender, Receiver};

struct UserData {
    format: spa::param::video::VideoInfoRaw,
    tx: Sender<Frame>,
}

pub struct PipeWireStream {
    pub receiver: Receiver<Frame>,
}

impl PipeWireStream {
    pub fn new(node_id: u32, fd: OwnedFd) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let (tx, rx) = bounded(5);
        
        std::thread::spawn(move || {
            pw::init();
            let mainloop = pw::main_loop::MainLoopRc::new(None).unwrap();
            let context = pw::context::ContextRc::new(&mainloop, None).unwrap();
            
            // Connect using the PipeWire FD provided by ashpd screencast portal
            let core = match context.connect_fd_rc(fd, None) {
                Ok(c) => c,
                Err(_) => {
                    eprintln!("Failed to connect to pipewire via fd");
                    return;
                }
            };
            
            let data = UserData {
                format: Default::default(),
                tx,
            };

            let stream = pw::stream::StreamBox::new(
                &core,
                "vrec-capture",
                properties! {
                    *pw::keys::MEDIA_TYPE => "Video",
                    *pw::keys::MEDIA_CATEGORY => "Capture",
                    *pw::keys::MEDIA_ROLE => "Screen",
                },
            ).unwrap();

            let _listener = stream
                .add_local_listener_with_user_data(data)
                .param_changed(|_, user_data, id, param| {
                    let Some(param) = param else { return; };
                    if id != pw::spa::param::ParamType::Format.as_raw() { return; }
                    
                    let (media_type, media_subtype) = match pw::spa::param::format_utils::parse_format(param) {
                        Ok(v) => v,
                        Err(_) => return,
                    };
                    
                    if media_type != pw::spa::param::format::MediaType::Video || media_subtype != pw::spa::param::format::MediaSubtype::Raw {
                        return;
                    }
                    
                    user_data.format.parse(param).unwrap();
                    println!("Wayland Stream Negotiated: {}x{}", user_data.format.size().width, user_data.format.size().height);
                })
                .process(|stream, user_data| {
                    match stream.dequeue_buffer() {
                        None => {},
                        Some(mut buffer) => {
                            let datas = buffer.datas_mut();
                            if datas.is_empty() { return; }
                            let data = &mut datas[0];
                            
                            let fd = data.fd();
                            let stride = data.chunk().stride() as u32;
                            let offset = data.chunk().offset();
                            let frame = if fd > 0 {
                                Frame::DmaBuf {
                                    width: user_data.format.size().width,
                                    height: user_data.format.size().height,
                                    format: 0x34325241, // DRM_FORMAT_ARGB8888
                                    modifier: user_data.format.modifier(), 
                                    fd,
                                    stride,
                                    offset,
                                }
                            } else if let Some(slice) = data.data() {
                                Frame::Raw {
                                    width: user_data.format.size().width,
                                    height: user_data.format.size().height,
                                    stride,
                                    data: slice.to_vec(),
                                }
                            } else {
                                return;
                            };
                            let _ = user_data.tx.try_send(frame);
                        }
                    }
                })
                .register().unwrap();
                
            let obj = pw::spa::pod::object!(
                pw::spa::utils::SpaTypes::ObjectParamFormat,
                pw::spa::param::ParamType::EnumFormat,
                pw::spa::pod::property!(
                    pw::spa::param::format::FormatProperties::MediaType,
                    Id,
                    pw::spa::param::format::MediaType::Video
                ),
                pw::spa::pod::property!(
                    pw::spa::param::format::FormatProperties::MediaSubtype,
                    Id,
                    pw::spa::param::format::MediaSubtype::Raw
                ),
            );
            
            let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
                std::io::Cursor::new(Vec::new()),
                &pw::spa::pod::Value::Object(obj),
            ).unwrap().0.into_inner();
            let mut params = [Pod::from_bytes(&values).unwrap()];

            stream.connect(
                spa::utils::Direction::Input,
                Some(node_id),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut params,
            ).unwrap();

            mainloop.run();
        });
        
        Ok(Self { receiver: rx })
    }

    pub fn next_frame(&mut self) -> Result<Frame, Box<dyn std::error::Error + Send + Sync>> {
        self.receiver.recv().map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}
