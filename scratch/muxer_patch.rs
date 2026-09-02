use std::fs;

fn main() {
    let mut code = fs::read_to_string("src/muxer.rs").unwrap();
    code = code.replace(
        "pub unsafe fn new(path: &str, codec_ctx: *mut AVCodecContext) -> Result<Self, String> {",
        "pub unsafe fn new(path: &str, codec_ctx: *mut AVCodecContext, audio_codec_ctx: Option<*mut AVCodecContext>) -> Result<Self, String> {"
    );
    
    code = code.replace(
        r#"            let ret = avcodec_parameters_from_context((*stream).codecpar, codec_ctx);
            if ret < 0 {
                return Err("Failed to copy codec parameters".into());
            }"#,
        r#"            (*stream).id = 0;
            let ret = avcodec_parameters_from_context((*stream).codecpar, codec_ctx);
            if ret < 0 {
                return Err("Failed to copy video codec parameters".into());
            }
            
            if let Some(actx) = audio_codec_ctx {
                let astream = avformat_new_stream(fmt_ctx, std::ptr::null());
                if !astream.is_null() {
                    (*astream).id = 1;
                    if avcodec_parameters_from_context((*astream).codecpar, actx) < 0 {
                        eprintln!("Failed to copy audio codec parameters");
                    }
                }
            }"#
    );
    
    fs::write("src/muxer.rs", code).unwrap();
}
