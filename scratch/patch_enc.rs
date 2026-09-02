use std::fs;

fn main() {
    let mut code = fs::read_to_string("src/encoder.rs").unwrap();
    code = code.replace(
        "(*codec_ctx).channel_layout = if channels == 2 { AV_CH_LAYOUT_STEREO } else { AV_CH_LAYOUT_MONO } as u64;",
        "av_channel_layout_default(&mut (*codec_ctx).ch_layout, channels);"
    );
    
    // For SwrContext, swr_alloc_set_opts is deprecated in favor of swr_alloc_set_opts2
    // But swr_alloc_set_opts2 requires passing pointer to AVChannelLayout.
    code = code.replace(
        r#"            let mut swr_ctx = swr_alloc();
            av_opt_set_int(swr_ctx as _, c"in_channel_layout".as_ptr(), (*codec_ctx).channel_layout as i64, 0);
            av_opt_set_int(swr_ctx as _, c"in_sample_rate".as_ptr(), sample_rate as i64, 0);
            av_opt_set_sample_fmt(swr_ctx as _, c"in_sample_fmt".as_ptr(), AVSampleFormat::AV_SAMPLE_FMT_FLT, 0);
            
            av_opt_set_int(swr_ctx as _, c"out_channel_layout".as_ptr(), (*codec_ctx).channel_layout as i64, 0);
            av_opt_set_int(swr_ctx as _, c"out_sample_rate".as_ptr(), sample_rate as i64, 0);
            av_opt_set_sample_fmt(swr_ctx as _, c"out_sample_fmt".as_ptr(), AVSampleFormat::AV_SAMPLE_FMT_FLTP, 0);
            
            swr_init(swr_ctx);"#,
        r#"            let mut swr_ctx: *mut SwrContext = std::ptr::null_mut();
            swr_alloc_set_opts2(
                &mut swr_ctx,
                &(*codec_ctx).ch_layout,
                AVSampleFormat::AV_SAMPLE_FMT_FLTP,
                sample_rate,
                &(*codec_ctx).ch_layout,
                AVSampleFormat::AV_SAMPLE_FMT_FLT,
                sample_rate,
                0,
                std::ptr::null_mut()
            );
            swr_init(swr_ctx);"#
    );
    
    code = code.replace("(*in_frame).channel_layout = (*self.codec_ctx).channel_layout;", "(*in_frame).ch_layout = (*self.codec_ctx).ch_layout;");
    code = code.replace("(*out_frame).channel_layout = (*self.codec_ctx).channel_layout;", "(*out_frame).ch_layout = (*self.codec_ctx).ch_layout;");
    code = code.replace("(*enc_frame).channel_layout = (*self.codec_ctx).channel_layout;", "(*enc_frame).ch_layout = (*self.codec_ctx).ch_layout;");

    fs::write("src/encoder.rs", code).unwrap();
}
