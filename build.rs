// build.rs：Tonic的构建脚本，编译.proto生成Rust代码
fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 编译proto目录下的所有.proto文件，输出到OUT_DIR（Cargo自动生成）
    tonic_build::compile_protos("proto/helloworld.proto")?;

    // 如果有多个.proto，用这个（glob模式）
    // tonic_build::configure()
    //     .compile(&["proto/helloworld.proto", "proto/other.proto"], &["src"])?;

    // 或编译整个目录
    // tonic_build::compile_protos("proto")?;

    Ok(()) // 成功返回
}
