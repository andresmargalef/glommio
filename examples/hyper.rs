#[macro_use]
extern crate lazy_static;

// Example on how to use the Hyper server in !Send mode.
// The clients are harder, see https://github.com/hyperium/hyper/issues/2341 for details
//
// Essentially what we do is we wrap our types around the Tokio traits. The
// !Send limitation makes it harder to deal with high level hyper primitives but
// it works in the end.
mod hyper_compat {
    use futures_lite::{AsyncRead, AsyncWrite, Future};
    use hyper::service::service_fn;
    use std::{
        net::SocketAddr,
        pin::Pin,
        task::{Context, Poll},
    };
    use hyper::service::make_service_fn;

    use glommio::{
        enclose,
        net::{TcpListener, TcpStream},
        sync::Semaphore,
        Local,
        Task,
    };
    use hyper::{Body, Request, Response};
    use std::{io, rc::Rc};

    #[derive(Clone)]
    struct HyperExecutor;

    impl<F> hyper::rt::Executor<F> for HyperExecutor
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        fn execute(&self, fut: F) {
            Task::local(fut).detach();
        }
    }

    struct HyperStream(pub TcpStream);
    impl tokio::io::AsyncRead for HyperStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &mut [u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.0).poll_read(cx, buf)
        }
    }

    impl tokio::io::AsyncWrite for HyperStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.0).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_flush(cx)
        }

        fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
            Pin::new(&mut self.0).poll_close(cx)
        }
    }

    pub(crate) async fn serve_http<S, F, R, A>(
        addr: A,
        service: S,
        max_connections: usize,
    ) -> io::Result<()>
    where
        S: FnMut(Request<Body>) -> F + 'static + Copy,
        F: Future<Output = Result<Response<Body>, R>> + 'static,
        R: std::error::Error + 'static + Send + Sync,
        A: Into<SocketAddr>,
    {
        let listener = TcpListener::bind(addr.into())?;
        let conn_control = Rc::new(Semaphore::new(max_connections as _));
        loop {
            match listener.accept().await {
                Err(x) => {
                    return Err(x.into());
                }
                Ok(stream) => {
                    let addr = stream.local_addr().unwrap();
                    Local::local(enclose!{(conn_control) async move {
                        let _permit = conn_control.acquire_permit(1).await;
                        let make_service = make_service_fn(|_| async { Ok::<_, Infallible>(service_fn(service)) });
                        let builder = hyper::Server::builder(HyperStream(stream)).executor(HyperExecutor);
                        if let Err(x) = builder.serve(make_service).await {
                            panic!("Stream from {:?} failed with error {:?}", addr, x);
                        }
                    }}).detach();
                }
            }
        }
    }
}

use glommio::LocalExecutorBuilder;
use hyper::{Body, Method, Request, Response, StatusCode};
use std::convert::Infallible;

use hyper::{
    header::{HeaderValue, CONNECTION},
    Uri, client::HttpConnector, Client
};

pub type HttpClient = Client<HttpConnector>;

lazy_static! {
    pub static ref STATIC_CLIENT: HttpClient = Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(300)
        .http1_max_buf_size(1024*1024)
        .build_http();
}

pub async fn aggregate(body: hyper::Body) -> Result<hyper::Body, hyper::Error> {
    let mut bufs = Vec::new();

    futures_util::pin_mut!(body);
    while let Some(buf) = body.data().await {
        let buf = buf;
        bufs.push(buf);
    }

    let stream = futures_util::stream::iter(bufs);

    let body = Body::wrap_stream(stream);

    Ok(body)
}

async fn hyper_demo(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let (mut parts, body) = req.into_parts();

    // this line is to fetch all the body before sending to clients
    // If store&forward not works, comment this line
    let body = aggregate(body).await?;

    parts.uri = Uri::from_static("http://consumer1:8080/consume1");
    parts
        .headers
        .insert(CONNECTION, HeaderValue::from_static("keep-alive"));
    let req_fast = Request::from_parts(parts, body);
    let mapped = STATIC_CLIENT.request(req_fast).await?;
    Ok(mapped)
    /*
    match (req.method(), req.uri().path()) {
        (&Method::GET, "/hello") => Ok(Response::new(Body::from("world"))),
        (&Method::GET, "/world") => Ok(Response::new(Body::from("hello"))),
        _ => Ok(Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("notfound"))
            .unwrap()),
    }*/
}

fn main() {
    // Issue curl -X GET http://127.0.0.1:8000/hello or curl -X GET http://127.0.0.1:8000/world to
    // see it in action
    let handle = LocalExecutorBuilder::new()
        .spawn(|| async move {
            hyper_compat::serve_http(([0, 0, 0, 0], 8000), hyper_demo, 1)
                .await
                .unwrap();
        })
        .unwrap();

    handle.join().unwrap();
}
