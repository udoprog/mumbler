use tracing::Level;
use tracing_wasm::WASMLayerConfigBuilder;

fn main() {
    let config = WASMLayerConfigBuilder::new()
        .set_max_level(Level::INFO)
        .build();

    tracing_wasm::set_as_global_default_with_config(config);
    tracing::trace!("Started up");
    yew::Renderer::<web::App>::new().render();
}
