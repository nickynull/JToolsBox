fn main() {
    // 网页资源不再编译期嵌入：运行期从 <程序资产根>/webapps/<功能名>/ 读取，
    // 所以这里既不需要扫描 web/*/dist，也不会因为前端产物变化触发 Rust 重编。
    tauri_build::build()
}
