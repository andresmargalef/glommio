use crate::commons::{CustomResult, STATIC_CLIENT};
use hyper::{
    header::{HeaderValue, CONNECTION},
    Body, Request, Response, Uri,
};

use http_body::Body as HttpBody;

pub type CustomResult<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

lazy_static! {
    pub static ref STATIC_CLIENT: HttpClient = Client::builder()
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .pool_max_idle_per_host(300)
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

#[inline]
pub async fn direct_routing(req: Request<Body>) -> CustomResult<Response<Body>> {
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
}
