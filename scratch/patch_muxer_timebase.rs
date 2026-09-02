use std::fs;
fn main() {
    let mut code = fs::read_to_string("src/muxer.rs").unwrap();
    code = code.replace(
        "if ret < 0 {\n                return Err(\"Failed to copy video codec parameters\".into());\n            }",
        "if ret < 0 {\n                return Err(\"Failed to copy video codec parameters\".into());\n            }\n            (*stream).time_base = (*codec_ctx).time_base;"
    );
    code = code.replace(
        "eprintln!(\"Failed to copy audio codec parameters\");\n                    }",
        "eprintln!(\"Failed to copy audio codec parameters\");\n                    }\n                    (*astream).time_base = (*actx).time_base;"
    );
    fs::write("src/muxer.rs", code).unwrap();
}
