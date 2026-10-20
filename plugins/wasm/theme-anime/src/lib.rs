//! theme-anime 插件：站点前端主题（晴空与樱花的动漫风）。全部内容在前端样式表 `web/plugins/theme-anime/src/theme.css`，
//! 组件本身不做任何事。

pt_plugin_sdk::world!("theme-anime");

use exports::pt::plugin::lifecycle::Guest;
use pt_plugin_sdk::types::PluginError;

struct ThemeAnime;

impl Guest for ThemeAnime {
    fn init() -> Result<(), PluginError> {
        Ok(())
    }
}

export!(ThemeAnime);
