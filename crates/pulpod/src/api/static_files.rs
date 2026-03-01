use axum::{
    body::Body,
    http::{header, response::Builder},
    response::Response,
};

use super::embed::Asset;

pub async fn serve(uri: axum::http::Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Try the exact path first, otherwise SPA fallback to index.html
    let (name, content) = Asset::get(path)
        .map(|c| (path, c))
        .or_else(|| Asset::get("index.html").map(|c| ("index.html", c)))
        .expect("index.html must be embedded in the binary");

    let mime = mime_guess::from_path(name).first_or_octet_stream();
    build_response(mime.as_ref(), &content.data)
}

fn build_response(content_type: &str, data: &[u8]) -> Response {
    Builder::new()
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(data.to_vec()))
        .expect("static response build cannot fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{StatusCode, Uri};

    #[tokio::test]
    async fn test_serve_missing_file_falls_back_to_index() {
        let uri: Uri = "/nonexistent.js".parse().unwrap();
        let resp = serve(uri).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_serve_root_path() {
        let uri: Uri = "/".parse().unwrap();
        let resp = serve(uri).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_serve_exact_file() {
        let uri: Uri = "/robots.txt".parse().unwrap();
        let resp = serve(uri).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_serve_index_html_directly() {
        let uri: Uri = "/index.html".parse().unwrap();
        let resp = serve(uri).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_asset_get_nonexistent() {
        assert!(Asset::get("nonexistent").is_none());
    }

    #[test]
    fn test_asset_get_existing() {
        assert!(Asset::get("index.html").is_some());
        assert!(Asset::get("robots.txt").is_some());
    }

    #[test]
    fn test_asset_embed_lists_files() {
        let names: Vec<_> = Asset::iter().collect();
        assert!(names.iter().any(|n| n.as_ref() == "index.html"));
        assert!(names.iter().any(|n| n.as_ref() == "robots.txt"));
    }

    #[test]
    fn test_build_response_html() {
        let resp = build_response("text/html", b"<html></html>");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_build_response_js() {
        let resp = build_response("application/javascript", b"console.log('hi')");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[test]
    fn test_build_response_octet_stream() {
        let resp = build_response("application/octet-stream", b"\x00\x01\x02");
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
