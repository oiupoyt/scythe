use std::fs;
fn main() {
    let mut code = fs::read_to_string("src/ring.rs").unwrap();
    code = code.replace(
        "pub fn is_keyframe(&self) -> bool {",
        "pub fn stream_index(&self) -> i32 {
        unsafe { (*self.ptr).stream_index }
    }
    
    pub fn set_stream_index(&mut self, idx: i32) {
        unsafe { (*self.ptr).stream_index = idx; }
    }
    
    pub fn is_keyframe(&self) -> bool {"
    );
    fs::write("src/ring.rs", code).unwrap();
}
