use ffmpeg_next::ffi::*;
fn main() {
    unsafe {
        let codec = avcodec_find_encoder(AVCodecID::AV_CODEC_ID_AAC);
        if codec.is_null() {
            println!("AAC encoder not found");
        } else {
            println!("AAC encoder found");
        }
    }
}
