use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../web/build/"]
pub(super) struct Asset;
