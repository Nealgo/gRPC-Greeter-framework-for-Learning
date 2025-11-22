use tokio::sync::mpsc;
use tonic::{Request, Response, Status, transport::Server};

use hello_world::greeter_server::{Greeter, GreeterServer};
use hello_world::{HelloReply, HelloRequest};

use tokio::time::{Duration, sleep};
use tokio_stream::wrappers::ReceiverStream;



pub mod hello_world {
    tonic::include_proto!("helloworld");  // 这部分代码是自动生成的
}

#[derive(Debug, Default)]
pub struct MyGreeter {}

#[tonic::async_trait]
impl Greeter for MyGreeter {
    async fn say_hello(
        &self,
        request: Request<HelloRequest>,
    ) -> Result<Response<HelloReply>, Status> {
        println!("Got a request: {:?}", request);

        // 1. 从 Tonic 的包装中取出我们定义的请求消息体
        let name = request.into_inner().name;

        // 2. 构建我们定义的响应消息体
        let reply = HelloReply {
            message: format!("Hello {}!", name),
        };

        // 3. 将响应消息体包装进 Tonic 的 Response 中并返回
        Ok(Response::new(reply))
    }

    type SayHelloStreamStream = ReceiverStream<Result<HelloReply, Status>>;

    async fn say_hello_stream(
    &self,
    request: Request<HelloRequest>,
    ) -> Result<Response<Self::SayHelloStreamStream>, Status> {
        let input = request.into_inner().name;
        
        // 简单的参数解析
        let parts: Vec<&str> = input.split(':').collect();
        let count = parts.get(0).unwrap_or(&"100").parse::<usize>().unwrap_or(100);
        let size = parts.get(1).unwrap_or(&"10").parse::<usize>().unwrap_or(10);

        // 创建一个指定大小的 Payload 字符串
        let payload = "a".repeat(size);

        // 增大 channel 容量以避免服务端发送被阻塞，影响纯粹的生成速度测试
        // 但要注意，如果 channel 满了，反映的就是真实的网络/客户端反压瓶颈
        let (tx, rx) = mpsc::channel(1000);// 消息的数量上线是1000，也就是HelloReply的数量上限。

        tokio::spawn(async move {
            for i in 0..count {
                let reply = HelloReply {
                    // 模拟真实数据：带序号 + Payload
                    message: format!("Seq-{}::{}", i, payload),
                };
                
                if tx.send(Ok(reply)).await.is_err() {
                    break; // 客户端断开
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "[::1]:50051".parse()?;
    let greeter = MyGreeter::default();
    println!("Profiling Server listening on {}", addr);

    // 设置最大消息大小为 64MB (默认是 4MB)
    let max_msg_size = 64 * 1024 * 1024; 
     
    Server::builder()
        .add_service(
            GreeterServer::new(greeter)
                // 关键修改：同时放宽接收(decoding)和发送(encoding)的限制
                .max_decoding_message_size(max_msg_size)
                .max_encoding_message_size(max_msg_size)
        )
        .serve(addr)
        .await?;
 
    Ok(())
}
