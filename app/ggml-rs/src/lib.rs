mod backend;
mod compute_graph;
mod device;
mod ggml_sys;
mod execution_manager;

#[unsafe(no_mangle)]
pub extern "C" fn ggml_backend_ttx_reg() -> ggml_sys::ggml_backend_reg_t {
    tracing_subscriber::util::SubscriberInitExt::init(
        tracing_subscriber::layer::SubscriberExt::with(
            tracing_subscriber::layer::SubscriberExt::with(
                tracing_subscriber::registry(),
                tracing_subscriber::fmt::layer(),
            ),
            tracing_subscriber::filter::EnvFilter::from_default_env(),
        ),
    );

    &raw mut backend::BACKEND_REG
}
