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
        let name = request.into_inner().name;

        // 1. 创建一个异步通道 (Channel)
        // tx: 发送端, rx: 接收端。4 是通道的缓冲区大小。
        let (tx, rx) = mpsc::channel(4);

        // 2. 启动一个新的异步任务 (Task) 来生成并发送数据
        tokio::spawn(async move {
            for i in 1..=3 {
                let message = format!("Hello, {}! ({}/3)", name, i);

                // 3. 通过通道的发送端(tx)发送消息
                if tx.send(Ok(HelloReply { message })).await.is_err() {
                    // 如果发送失败 (通常是客户端断开了连接), 就退出任务
                    break;
                }
                sleep(Duration::from_secs(1)).await; // 模拟耗时操作
            }
        });

        // 4. 立刻返回一个包含通道接收端(rx)的响应
        Ok(Response::new(ReceiverStream::new(rx)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. 定义并解析监听地址
    let addr = "[::1]:50051".parse()?;

    // 2. 创建我们服务逻辑的实例
    let greeter = MyGreeter::default();

    println!("Server listening on {}", addr);

    // 3. 构建并启动服务器
    Server::builder()
        // 添加服务，注意这里是 GreeterServer::new(greeter)
        // 用 tonic 生成的 GreeterServer 来包装我们的 MyGreeter 实例
        .add_service(GreeterServer::new(greeter))
        // 绑定到地址并开始监听
        .serve(addr)
        .await?;

    Ok(())
}
