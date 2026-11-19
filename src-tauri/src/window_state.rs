//! 主窗口的位置与大小持久化。
//!
//! 数据写在 exe 同级 `config.json` 的 `window_state` 字段里（与主题、关闭行为同一份配置），
//! 启动时在窗口 `show()` 之前恢复，运行中按窗口事件去抖保存。
//!
//! 三条约定：
//! - 尺寸存**逻辑像素**（换到不同缩放比例的显示器时"看起来一样大"），位置存**物理像素**（屏幕坐标）；
//! - 最大化时**不**覆盖已存的还原几何，只翻 `maximized` 标记，否则"还原"出来的是全屏尺寸；
//! - 位置失效（显示器被拔掉 / 分辨率变小）时退化为主屏居中，绝不把窗口丢到看不见的地方。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalSize, PhysicalPosition, Window};

/// 最小窗口尺寸（逻辑像素），与 tauri.conf.json 的 minWidth / minHeight 保持一致
pub const MIN_WIDTH: f64 = 640.0;
pub const MIN_HEIGHT: f64 = 480.0;

/// 判定"窗口还看得见"的最小可见边长（逻辑像素）
const MIN_VISIBLE: f64 = 100.0;

/// 拖动 / 缩放结束后静默这么久才落盘，避免拖动过程中反复写 config.json
const SAVE_DEBOUNCE: Duration = Duration::from_millis(600);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct WindowState {
    /// 逻辑像素
    pub width: f64,
    /// 逻辑像素
    pub height: f64,
    /// 物理像素（屏幕坐标）
    pub x: i32,
    /// 物理像素（屏幕坐标）
    pub y: i32,
    pub maximized: bool,
}

impl WindowState {
    /// 浮点尺寸经过逻辑/物理像素来回换算会有零点几的误差，比较时留容差，
    /// 否则恢复后立刻触发的事件会被当成"有变化"再写一次盘
    pub fn same_geometry(&self, other: &WindowState) -> bool {
        self.maximized == other.maximized
            && (self.width - other.width).abs() < 0.5
            && (self.height - other.height).abs() < 0.5
            && (self.x - other.x).abs() <= 1
            && (self.y - other.y).abs() <= 1
    }
}

/// 启动时恢复窗口几何；没有记录或数据明显非法时什么都不做，沿用 tauri.conf.json 的默认值
pub fn restore(window: &Window, state: Option<&WindowState>) {
    let Some(state) = state else {
        return;
    };
    if !state.width.is_finite()
        || !state.height.is_finite()
        || state.width <= 0.0
        || state.height <= 0.0
    {
        return;
    }

    // 旧版本或手改配置可能存了过小的尺寸，这里再钳一次
    let width = state.width.max(MIN_WIDTH);
    let height = state.height.max(MIN_HEIGHT);

    let _ = window.set_size(LogicalSize::new(width, height));
    if let Some(position) = resolve_position(window, state.x, state.y, width, height) {
        let _ = window.set_position(position);
    }
    // 必须先把还原尺寸摆好再最大化，否则"还原"按钮回来的是最大化时的尺寸
    if state.maximized {
        let _ = window.maximize();
    }
}

/// 校验上次的位置是否还落在某块屏幕的可见范围内；
/// 落在屏幕外时退化为在主屏居中，返回 None 表示完全拿不到屏幕信息（放弃设置位置）
fn resolve_position(
    window: &Window,
    x: i32,
    y: i32,
    width: f64,
    height: f64,
) -> Option<PhysicalPosition<i32>> {
    let Ok(monitors) = window.available_monitors() else {
        return None;
    };
    if monitors.is_empty() {
        return Some(PhysicalPosition::new(x, y));
    }

    // 屏幕坐标是物理像素，用当前缩放把逻辑尺寸近似成物理尺寸再求交集
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
    let w = (width * scale).round() as i32;
    let h = (height * scale).round() as i32;
    let need = (MIN_VISIBLE * scale).round() as i32;

    let visible = monitors.iter().any(|monitor| {
        let pos = monitor.position();
        let size = monitor.size();
        let dx = (x + w).min(pos.x + size.width as i32) - x.max(pos.x);
        let dy = (y + h).min(pos.y + size.height as i32) - y.max(pos.y);
        dx >= need && dy >= need
    });
    if visible {
        return Some(PhysicalPosition::new(x, y));
    }

    let primary = window.primary_monitor().ok().flatten();
    let target = primary.as_ref().or_else(|| monitors.first())?;
    let pos = target.position();
    let size = target.size();
    Some(PhysicalPosition::new(
        pos.x + (size.width as i32 - w) / 2,
        pos.y + (size.height as i32 - h) / 2,
    ))
}

/// 采集窗口当前几何；最小化时返回 None（尺寸不可信），最大化时沿用上一次的还原几何
fn capture(window: &Window, previous: Option<&WindowState>) -> Option<WindowState> {
    if window.is_minimized().unwrap_or(false) {
        return None;
    }

    let maximized = window.is_maximized().unwrap_or(false);
    if !maximized {
        let scale = window.scale_factor().ok().filter(|scale| *scale > 0.0)?;
        let size = window.inner_size().ok()?;
        let position = window.outer_position().ok()?;
        return Some(WindowState {
            width: size.width as f64 / scale,
            height: size.height as f64 / scale,
            x: position.x,
            y: position.y,
            maximized: false,
        });
    }

    // 最大化时 inner_size / outer_position 都是全屏值，不能覆盖已存的还原几何
    previous.map(|prev| WindowState {
        maximized: true,
        ..prev.clone()
    })
}

/// 读取窗口当前的还原几何（最小化 / 最大化时返回 None）。
/// 首次启动没有存档时用它当基线，之后（例如一启动就最大化）才有"还原尺寸"可参考
pub fn capture_current(window: &Window) -> Option<WindowState> {
    capture(window, None)
}

/// 去抖保存器：窗口事件只更新内存里的待写值并唤醒常驻任务，
/// 静默 600ms 才写一次 config.json，拖动窗口不会造成密集磁盘 IO
pub struct WindowStateSaver {
    latest: Mutex<Option<WindowState>>,
    dirty: AtomicBool,
    notify: tokio::sync::Notify,
}

impl WindowStateSaver {
    pub fn new(initial: Option<WindowState>) -> Self {
        Self {
            latest: Mutex::new(initial),
            dirty: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    /// 窗口 Moved / Resized / 缩放变化时调用，有变化才入队
    pub fn queue(&self, window: &Window) {
        let previous = self.latest.lock().ok().and_then(|slot| slot.clone());
        let Some(next) = capture(window, previous.as_ref()) else {
            return;
        };
        if let Some(prev) = previous.as_ref() {
            if prev.same_geometry(&next) {
                return;
            }
        }

        if let Ok(mut slot) = self.latest.lock() {
            *slot = Some(next);
        }
        self.dirty.store(true, Ordering::SeqCst);
        self.notify.notify_one();
    }

    /// 立刻落盘，退出前调用，避免去抖窗口里的最后一次改动丢失
    pub fn flush(&self, app: &AppHandle) {
        if !self.dirty.swap(false, Ordering::SeqCst) {
            return;
        }
        let latest = self.latest.lock().ok().and_then(|slot| slot.clone());
        if let Some(state) = latest {
            crate::persist_window_state(app, &state);
        }
    }

    /// 常驻的落盘任务：被唤醒后等静默期结束再写，连续事件自动合并成一次 IO
    pub fn spawn_flusher(self: Arc<Self>, app: AppHandle) {
        tauri::async_runtime::spawn(async move {
            loop {
                self.notify.notified().await;
                tokio::time::sleep(SAVE_DEBOUNCE).await;
                self.flush(&app);
            }
        });
    }
}
