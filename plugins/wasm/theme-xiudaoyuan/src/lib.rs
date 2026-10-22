//! theme-xiudaoyuan 插件：站点前端主题「修道院」（取材《让仙门再次伟大》，白玉京配色）。
//! 样式在 `web/plugins/theme-xiudaoyuan/src/theme.css`，等级等称呼由清单 `[frontend.term_overrides]` 覆盖；组件本身不做任何事。

pt_plugin_sdk::world!("theme-xiudaoyuan");

use exports::pt::plugin::lifecycle::Guest;
use pt_plugin_sdk::types::PluginError;

struct ThemeXiudaoyuan;

impl Guest for ThemeXiudaoyuan {
    fn init() -> Result<(), PluginError> {
        Ok(())
    }
}

export!(ThemeXiudaoyuan);
