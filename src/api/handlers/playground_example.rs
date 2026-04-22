use axum::response::Html;

pub async fn playground_example() -> Html<&'static str> {
    Html(include_str!("../../../static/playground-example.html"))
}
