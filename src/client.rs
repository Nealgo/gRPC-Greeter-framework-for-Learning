use hello_world::greeter_client::GreeterClient;
use hello_world::HelloRequest;
use tokio_stream::StreamExt;

pub mod hello_world {
    tonic::include_proto!("helloworld");
}

//在服务端，我们使用的是生成的 GreeterServer 和 Greeter Trait。
//在这里，我们使用的是 GreeterClient。这是 tonic-build 自动为我们生成的客户端存根 (stub)。

//这个 GreeterClient 结构体提供了一系列与 .proto 文件中定义的 rpc 方法同名的异步函数（如 say_hello 和 say_hello_stream）。
//它为我们封装了所有底层的网络通信、序列化和反序列化的复杂工作。

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = GreeterClient::connect("http://[::1]:50051").await?;

    // Unary RPC: 
    //1 这里完成消息的封装
    let request = tonic::Request::new(HelloRequest {
        name: "Alice".into(),// into函数表示的是类型转换
    });

    //2 这里进行调用
    let response = client.say_hello(request).await?;

    //可视化反馈
    println!("Unary Response: {}", response.into_inner().message); // message表示结构体字段，
    // into_inner()表示的是提取类型


    // Streaming RPC: SayHelloStream
    let request = tonic::Request::new(HelloRequest {
        name: "Bob".into(),
    });

    let mut stream = client.say_hello_stream(request).await?.into_inner();// Response<Streaming<HelloReply>>

    while let Some(response) = stream.next().await {
        match response {
            Ok(reply) => {
                println!("Stream Response: {}", reply.message);
            }
            Err(e) => eprintln!("Stream Error: {}", e),
        }
    }

    Ok(())
}

