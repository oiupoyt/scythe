use std::fs;

fn main() {
    let mut code = fs::read_to_string("src/capture/audio.rs").unwrap();
    code = code.replace(
        "pub fn new(sender: Sender<Vec<f32>>) -> Result<Self, Box<dyn std::error::Error>> {",
        "pub fn new(sender: Sender<Vec<f32>>) -> Result<(Self, u32, u16), Box<dyn std::error::Error>> {"
    );
    
    code = code.replace(
        "Ok(Self { _stream: stream })",
        "Ok((Self { _stream: stream }, sample_rate, channels))"
    );
    
    fs::write("src/capture/audio.rs", code).unwrap();
}
