use tracing::Level;
use tracing_wasm::WASMLayerConfigBuilder;

fn level_from_url() -> Option<Level> {
    let url = web_sys::window()?.location().href().ok()?;
    let url = url::Url::parse(&url).ok()?;
    let query = url.query_pairs().find(|(k, _)| k == "log")?.1;
    query.as_ref().parse().ok()
}

fn main() {
    let level = if let Some(level) = level_from_url() {
        level
    } else {
        Level::INFO
    };

    let config = WASMLayerConfigBuilder::new().set_max_level(level).build();

    tracing_wasm::set_as_global_default_with_config(config);
    tracing::trace!("Started up");
    yew::Renderer::<web::App>::new().render();
}
