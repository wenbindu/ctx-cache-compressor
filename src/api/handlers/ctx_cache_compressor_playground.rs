use axum::response::Html;

pub async fn ctx_cache_compressor_playground() -> Html<&'static str> {
    Html(include_str!(
        "../../../static/ctx-cache-compressor-playground.html"
    ))
}
