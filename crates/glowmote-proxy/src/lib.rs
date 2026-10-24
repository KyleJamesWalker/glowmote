//! HTTP front end for [`Controller`]: `POST /api/<command>` with the desktop app's arguments as
//! JSON, plus the app's UI with a shim that routes its Tauri calls here.

use std::sync::{Arc, LazyLock};

use axum::extract::{FromRequest, Request, State};
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::{Json, Router};
use glowmote_control::{Controller, Error, Radio};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

static UI: Dir = include_dir!("$CARGO_MANIFEST_DIR/../glowmote-app/ui");
const SHIM: &str = include_str!("tauri-shim.js");
const CSP: &str = "default-src 'self'";
static INDEX: LazyLock<String> = LazyLock::new(|| {
    let index = UI
        .get_file("index.html")
        .and_then(|f| f.contents_utf8())
        .expect("the app UI has an index.html");
    inject_shim(index)
});

/// Adds the shim ahead of the page's first script, so `window.__TAURI__` exists when it runs.
pub fn inject_shim(index: &str) -> String {
    let at = index
        .find("<script")
        .or_else(|| index.find("</head>"))
        .unwrap_or(0);
    let (head, tail) = index.split_at(at);
    format!("{head}<script src=\"tauri-shim.js\"></script>\n  {tail}")
}

pub fn router<R: Radio>(controller: Arc<Controller<R>>) -> Router {
    Router::new()
        .route("/api/status", get(status::<R>))
        .route("/api/scan_lights", post(scan_lights::<R>))
        .route("/api/connect", post(connect::<R>))
        .route("/api/disconnect", post(disconnect::<R>))
        .route("/api/info", post(info::<R>))
        .route("/api/power", post(power::<R>))
        .route("/api/color", post(color::<R>))
        .route("/api/scene", post(scene::<R>))
        .route("/api/diy", post(diy::<R>))
        .route("/api/diy_pixel", post(diy_pixel::<R>))
        .route("/api/diy_fill", post(diy_fill::<R>))
        .route("/api/{*command}", any(unknown_command))
        .method_not_allowed_fallback(method_not_allowed)
        .fallback(get(static_file))
        .with_state(controller)
}

pub struct ApiError(StatusCode, String);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        let status = match e {
            Error::Invalid(_) => StatusCode::BAD_REQUEST,
            Error::NotFound(_) => StatusCode::NOT_FOUND,
            Error::NotConnected => StatusCode::CONFLICT,
            Error::Device(_) => StatusCode::BAD_GATEWAY,
        };
        Self(status, e.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

type Reply = Result<Json<Value>, ApiError>;

fn reply<T: serde::Serialize>(result: glowmote_control::Result<T>) -> Reply {
    Ok(Json(json!({ "ok": result? })))
}

/// A JSON body whose rejections use the `{"error": ...}` shape. Requiring the JSON content type
/// makes cross-site pages go through a CORS preflight, which this server never grants.
struct Args<T>(T);

impl<S: Send + Sync, T: DeserializeOwned> FromRequest<S> for Args<T> {
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, ApiError> {
        match Json::<T>::from_request(req, state).await {
            Ok(Json(args)) => Ok(Self(args)),
            Err(rejection) => Err(ApiError(rejection.status(), rejection.body_text())),
        }
    }
}

type Ctl<R> = State<Arc<Controller<R>>>;

#[derive(Deserialize)]
struct NoArgs {}

#[derive(Deserialize)]
struct Name {
    name: String,
}

#[derive(Deserialize)]
struct On {
    on: bool,
}

#[derive(Deserialize)]
struct Color {
    rgb: String,
    brightness: u8,
}

#[derive(Deserialize)]
struct Scene {
    first: String,
    second: String,
    speed: u8,
    brightness: u8,
    reverse: bool,
}

#[derive(Deserialize)]
struct Pixel {
    index: u16,
    leds: u16,
    rgb: String,
    brightness: u8,
}

#[derive(Deserialize)]
struct Fill {
    leds: u16,
    rgb: String,
    brightness: u8,
}

async fn status<R: Radio>(State(c): Ctl<R>) -> Reply {
    Ok(Json(json!({ "ok": c.status().await })))
}

async fn scan_lights<R: Radio>(State(c): Ctl<R>, _: Args<NoArgs>) -> Reply {
    reply(c.scan_lights().await)
}

async fn connect<R: Radio>(State(c): Ctl<R>, Args(a): Args<Name>) -> Reply {
    reply(c.connect(&a.name).await)
}

async fn disconnect<R: Radio>(State(c): Ctl<R>, _: Args<NoArgs>) -> Reply {
    reply(c.disconnect().await)
}

async fn info<R: Radio>(State(c): Ctl<R>, _: Args<NoArgs>) -> Reply {
    reply(c.info().await)
}

async fn power<R: Radio>(State(c): Ctl<R>, Args(a): Args<On>) -> Reply {
    reply(c.power(a.on).await)
}

async fn color<R: Radio>(State(c): Ctl<R>, Args(a): Args<Color>) -> Reply {
    reply(c.color(&a.rgb, a.brightness).await)
}

async fn scene<R: Radio>(State(c): Ctl<R>, Args(a): Args<Scene>) -> Reply {
    reply(
        c.scene(&a.first, &a.second, a.speed, a.brightness, a.reverse)
            .await,
    )
}

async fn diy<R: Radio>(State(c): Ctl<R>, Args(a): Args<On>) -> Reply {
    reply(c.diy(a.on).await)
}

async fn diy_pixel<R: Radio>(State(c): Ctl<R>, Args(a): Args<Pixel>) -> Reply {
    reply(c.diy_pixel(a.index, a.leds, &a.rgb, a.brightness).await)
}

async fn diy_fill<R: Radio>(State(c): Ctl<R>, Args(a): Args<Fill>) -> Reply {
    reply(c.diy_fill(a.leds, &a.rgb, a.brightness).await)
}

async fn unknown_command(uri: Uri) -> ApiError {
    let command = uri.path().trim_start_matches("/api/");
    ApiError(StatusCode::NOT_FOUND, format!("unknown command {command}"))
}

async fn method_not_allowed() -> ApiError {
    ApiError(StatusCode::METHOD_NOT_ALLOWED, "method not allowed".into())
}

async fn static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let (body, mime): (&[u8], _) = match path {
        "" | "index.html" => (INDEX.as_bytes(), "text/html; charset=utf-8"),
        "tauri-shim.js" => (SHIM.as_bytes(), "text/javascript; charset=utf-8"),
        _ => match UI.get_file(path) {
            Some(file) => (file.contents(), mime(path)),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CONTENT_SECURITY_POLICY, CSP),
        ],
        body,
    )
        .into_response()
}

fn mime(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use glowmote_control::Link;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    type Packet = [u8; 16];

    struct FakeRadio;

    /// Acknowledges plain sends, fails unacknowledged ones and never confirms a scene.
    struct FakeLink;

    impl Radio for FakeRadio {
        type Found = String;
        type Link = FakeLink;

        fn name(found: &String) -> &str {
            found
        }

        async fn scan(&self, _: Duration) -> anyhow::Result<Vec<String>> {
            Ok(vec!["ISP-1".into()])
        }

        async fn connect(&self, _: String) -> anyhow::Result<FakeLink> {
            Ok(FakeLink)
        }
    }

    impl Link for FakeLink {
        fn name(&self) -> &str {
            "ISP-1"
        }

        async fn send(&mut self, _: &Packet) -> anyhow::Result<Option<String>> {
            Ok(Some("TURN".into()))
        }

        async fn send_unacked(&mut self, _: &Packet) -> anyhow::Result<()> {
            anyhow::bail!("radio off")
        }

        async fn send_scene(&mut self, _: &Packet, _: &[u8]) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn disconnect(self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn info(&mut self) -> anyhow::Result<glowmote_control::LightInfo> {
            anyhow::bail!("radio off")
        }
    }

    fn app() -> Router {
        router(Arc::new(Controller::with_radio(FakeRadio, Duration::ZERO)))
    }

    async fn call(
        app: &Router,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if body.is_some() {
            req = req.header(header::CONTENT_TYPE, "application/json");
        }
        let req = req.body(Body::from(body.unwrap_or("").to_owned())).unwrap();
        let res = app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[test]
    fn shim_goes_before_the_first_script() {
        let page = "<head>\n  <link>\n  <script src=\"main.js\" defer></script>\n</head>";
        assert_eq!(
            inject_shim(page),
            "<head>\n  <link>\n  <script src=\"tauri-shim.js\"></script>\n  \
             <script src=\"main.js\" defer></script>\n</head>"
        );
        assert!(inject_shim("<head></head>").starts_with("<head><script src=\"tauri-shim.js\">"));
    }

    #[tokio::test]
    async fn serves_the_app_ui_with_the_shim() {
        let app = app();
        let res = app
            .clone()
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.headers()[header::CONTENT_SECURITY_POLICY], CSP);
        let html = res.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&html).unwrap();
        let shim = html.find("tauri-shim.js").unwrap();
        assert!(shim < html.find("main.js").unwrap());

        for (path, mime) in [
            ("/tauri-shim.js", "text/javascript"),
            ("/main.js", "text/javascript"),
            ("/style.css", "text/css"),
        ] {
            let res = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{path}");
            assert!(
                res.headers()[header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with(mime)
            );
        }
        let res = app
            .oneshot(Request::get("/nope.js").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn runs_commands_and_wraps_results_in_ok() {
        let app = app();
        assert_eq!(
            call(&app, "GET", "/api/status", None).await,
            (StatusCode::OK, json!({ "ok": null }))
        );
        assert_eq!(
            call(&app, "POST", "/api/scan_lights", Some("{}")).await,
            (StatusCode::OK, json!({ "ok": ["ISP-1"] }))
        );
        assert_eq!(
            call(&app, "POST", "/api/connect", Some(r#"{"name":"ISP-1"}"#)).await,
            (StatusCode::OK, json!({ "ok": null }))
        );
        assert_eq!(
            call(&app, "GET", "/api/status", None).await.1,
            json!({ "ok": "ISP-1" })
        );
        assert_eq!(
            call(&app, "POST", "/api/power", Some(r#"{"on":true}"#)).await,
            (StatusCode::OK, json!({ "ok": "TURN" }))
        );
    }

    #[tokio::test]
    async fn errors_are_json_with_a_matching_status() {
        let app = app();
        let error = |status, message: &str| (status, json!({ "error": message }));
        assert_eq!(
            call(&app, "POST", "/api/power", Some(r#"{"on":true}"#)).await,
            error(StatusCode::CONFLICT, "not connected")
        );
        assert_eq!(
            call(&app, "POST", "/api/connect", Some(r#"{"name":"ISP-9"}"#)).await,
            error(StatusCode::NOT_FOUND, "ISP-9 not found; scan again")
        );
        assert_eq!(
            call(&app, "POST", "/api/nope", Some("{}")).await,
            error(StatusCode::NOT_FOUND, "unknown command nope")
        );
        assert_eq!(
            call(&app, "GET", "/api/power", None).await,
            error(StatusCode::METHOD_NOT_ALLOWED, "method not allowed")
        );

        call(&app, "POST", "/api/scan_lights", Some("{}")).await;
        call(&app, "POST", "/api/connect", Some(r#"{"name":"ISP-1"}"#)).await;
        assert_eq!(
            call(
                &app,
                "POST",
                "/api/diy_pixel",
                Some(r#"{"index":180,"leds":180,"rgb":"ff0000","brightness":50}"#)
            )
            .await,
            error(StatusCode::BAD_REQUEST, "index 180 is past the last LED")
        );
        assert_eq!(
            call(&app, "POST", "/api/diy", Some(r#"{"on":true}"#)).await,
            error(StatusCode::BAD_GATEWAY, "radio off")
        );
        assert_eq!(
            call(&app, "POST", "/api/scene", Some(r#"{"first":"ff0000","second":"00ff00","speed":50,"brightness":100,"reverse":false}"#)).await,
            error(StatusCode::BAD_GATEWAY, "the light did not confirm the scene colors")
        );
    }

    #[tokio::test]
    async fn bad_bodies_are_rejected_as_json() {
        let app = app();
        for (body, status) in [
            (Some(r#"{"on":"yes"}"#), StatusCode::UNPROCESSABLE_ENTITY),
            (Some("{"), StatusCode::BAD_REQUEST),
            (None, StatusCode::UNSUPPORTED_MEDIA_TYPE),
        ] {
            let (got, value) = call(&app, "POST", "/api/power", body).await;
            assert_eq!(got, status, "{body:?}");
            assert!(value["error"].is_string(), "{body:?}");
        }
    }
}
