//! `pt:plugin@1.0.0` 的宿主侧绑定（由 wasmtime bindgen 生成）。
//!
//! 生成代码内部使用 `unsafe`（TypedFunc::new_unchecked，类型已在 GuestIndices::load 中校验），
//! 因此在本模块放开 `unsafe_code`；手写代码不得放在本模块。
#![allow(unsafe_code, clippy::all, missing_debug_implementations)]

wasmtime::component::bindgen!({
    path: "../plugin-api/wit",
    world: "kernel",
    imports: { default: async },
    exports: { default: async },
});
