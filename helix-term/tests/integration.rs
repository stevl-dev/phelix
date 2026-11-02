#[cfg(feature = "integration")]
mod test {
    use helix_term::test_support as helpers;

    use helix_core::{syntax::config::AutoPairConfig, Selection};
    use helix_term::config::Config;

    use indoc::indoc;

    use helix_term::test_support::*;

    #[tokio::test(flavor = "multi_thread")]
    async fn hello_world() -> anyhow::Result<()> {
        test(("#[\n|]#", "ihello world<esc>", "hello world#[|\n]#")).await?;
        Ok(())
    }

    mod auto_pairs;
    mod command_line;
    mod commands;
    mod movement;
    mod splits;
    mod statusline;
    mod tabs;
}
