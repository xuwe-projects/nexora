//! DataTable 列顺序与列宽持久化的稳定模型和纯逻辑适配。
//!
//! 本模块不持有文件或进程状态；应用级偏好协调器负责读取、提交和广播布局 patch，
//! 本模块只按 `Column::key()` 合并当前代码列定义与已保存布局。

use std::collections::{HashMap, HashSet};

use gpui::Pixels;
use gpui_component::table::{Column, TableEvent};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// 一张可持久化 DataTable 的稳定组合身份。
///
/// `owner_id` 是 Feature 或 Window 的稳定 ID，`table_id` 是该所有者内部的稳定表格 ID。
/// 两者都不能依赖显示文本、本地化内容或数组下标。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub struct DataTableLayoutKey {
    /// Feature 或 Window 的稳定 ID。
    pub owner_id: String,
    /// 所有者内部表格的稳定 ID。
    pub table_id: String,
}

impl DataTableLayoutKey {
    /// 创建并校验表格布局组合身份。
    ///
    /// # Errors
    ///
    /// 任一 ID 为空、包含前后空白或包含路径分隔符时返回
    /// [`DataTableLayoutError::InvalidIdentity`]。
    pub fn new(
        owner_id: impl Into<String>,
        table_id: impl Into<String>,
    ) -> Result<Self, DataTableLayoutError> {
        let this = Self {
            owner_id: owner_id.into(),
            table_id: table_id.into(),
        };
        validate_identity("owner_id", &this.owner_id)?;
        validate_identity("table_id", &this.table_id)?;
        Ok(this)
    }

    /// 返回适合作为 TOML map key 的稳定编码。
    ///
    /// 两部分使用 `::` 分隔；构造阶段已禁止路径分隔符和空白边界，因此该编码可稳定跨重启。
    pub fn storage_key(&self) -> String {
        format!("{}::{}", self.owner_id, self.table_id)
    }
}

/// 单列持久化布局。
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct DataTableColumnLayout {
    /// 来自 `Column::key()` 的稳定列身份。
    pub key: String,
    /// 用户交互完成后的逻辑像素宽度。
    pub width: f32,
}

/// 一张 DataTable 的可持久化列布局。
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct DataTableLayout {
    /// 按用户当前顺序保存的列 key 与宽度。
    pub columns: Vec<DataTableColumnLayout>,
}

/// DataTable 布局恢复或事件转换失败时的结构化诊断。
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DataTableLayoutError {
    /// Feature、Window 或表格稳定 ID 不满足持久化身份约束。
    #[error("DataTable 布局 {field} `{value}` 必须是无前后空白且不含路径分隔符的非空稳定 ID")]
    InvalidIdentity {
        /// 发生错误的身份字段。
        field: &'static str,
        /// 调用方提供的非法值。
        value: String,
    },
    /// 当前代码列集合重复使用了同一个 `Column::key()`。
    #[error("DataTable 当前列集合包含重复 key `{key}`")]
    DuplicateCurrentColumnKey {
        /// 重复的稳定列 key。
        key: String,
    },
    /// 已保存布局重复声明了同一个稳定列 key。
    #[error("DataTable 已保存布局包含重复 key `{key}`")]
    DuplicateSavedColumnKey {
        /// 重复的稳定列 key。
        key: String,
    },
    /// 当前列约束在校验后仍无法填满可移动槽位，表示组件列模型内部不一致。
    #[error("DataTable 当前列约束无法填满可移动列槽位")]
    InternalColumnConstraintMismatch,
}

/// 把已保存布局安全应用到当前代码声明的列集合。
///
/// 已删除或未知列会被忽略；新增列按当前代码顺序补入尚未占用的可移动槽位。固定列和
/// `movable = false` 的列保留当前索引，`resizable = false` 的列保留代码宽度。可恢复宽度会
/// 被限制在当前列的 `min_width..=max_width` 范围。
///
/// # Errors
///
/// 当前列或已保存布局存在重复 key 时返回错误，并保持传入列集合不变。
pub fn apply_data_table_layout(
    columns: &mut Vec<Column>,
    layout: &DataTableLayout,
) -> Result<(), DataTableLayoutError> {
    validate_current_column_keys(columns)?;
    validate_saved_column_keys(layout)?;

    let saved = layout
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| (column.key.as_str(), (index, column.width)))
        .collect::<HashMap<_, _>>();
    let mut movable = columns
        .iter()
        .filter(|column| column.fixed.is_none() && column.movable)
        .cloned()
        .collect::<Vec<_>>();
    movable.sort_by_key(|column| {
        saved
            .get(column.key.as_ref())
            .map(|(index, _)| (0, *index))
            .unwrap_or((1, usize::MAX))
    });
    let mut movable = movable.into_iter();
    let mut restored = Vec::with_capacity(columns.len());
    for current in columns.iter() {
        let mut column = if current.fixed.is_some() || !current.movable {
            current.clone()
        } else {
            let Some(column) = movable.next() else {
                return Err(DataTableLayoutError::InternalColumnConstraintMismatch);
            };
            column
        };
        if column.resizable
            && let Some((_, width)) = saved.get(column.key.as_ref())
            && width.is_finite()
            && *width > 0.0
        {
            column.width = Pixels::from(*width).clamp(column.min_width, column.max_width);
        }
        restored.push(column);
    }
    *columns = restored;
    Ok(())
}

/// 从当前列集合生成可提交给偏好协调器的布局快照。
///
/// # Errors
///
/// 当前列集合包含重复 key 时返回 [`DataTableLayoutError::DuplicateCurrentColumnKey`]。
pub fn capture_data_table_layout(
    columns: &[Column],
) -> Result<DataTableLayout, DataTableLayoutError> {
    validate_current_column_keys(columns)?;
    Ok(DataTableLayout {
        columns: columns
            .iter()
            .map(|column| DataTableColumnLayout {
                key: column.key.to_string(),
                width: f32::from(column.width),
            })
            .collect(),
    })
}

/// 把原生 DataTable 列交互事件应用到业务 delegate 持有的列集合，并生成布局 patch。
///
/// `MoveColumn` 已由 `TableDelegate::move_column` 更新列顺序，本函数只捕获最终结果；
/// `ColumnWidthsChanged` 会先按当前列约束更新可缩放列。与列布局无关的事件返回 `Ok(None)`。
///
/// # Errors
///
/// 最终列集合包含重复 key 时返回错误。
pub fn data_table_layout_from_event(
    columns: &mut [Column],
    event: &TableEvent,
) -> Result<Option<DataTableLayout>, DataTableLayoutError> {
    match event {
        TableEvent::ColumnWidthsChanged(widths) => {
            apply_column_widths(columns, widths);
            capture_data_table_layout(columns).map(Some)
        }
        TableEvent::MoveColumn(_, _) => capture_data_table_layout(columns).map(Some),
        _ => Ok(None),
    }
}

fn apply_column_widths(columns: &mut [Column], widths: &[Pixels]) {
    columns
        .iter_mut()
        .zip(widths.iter().copied())
        .filter(|(column, _)| column.resizable)
        .for_each(|(column, width)| {
            column.width = width.clamp(column.min_width, column.max_width);
        });
}

fn validate_identity(field: &'static str, value: &str) -> Result<(), DataTableLayoutError> {
    if value.is_empty() || value.trim() != value || value.contains('/') || value.contains('\\') {
        return Err(DataTableLayoutError::InvalidIdentity {
            field,
            value: value.to_owned(),
        });
    }
    Ok(())
}

fn validate_current_column_keys(columns: &[Column]) -> Result<(), DataTableLayoutError> {
    let mut keys = HashSet::with_capacity(columns.len());
    for column in columns {
        if !keys.insert(column.key.as_ref()) {
            return Err(DataTableLayoutError::DuplicateCurrentColumnKey {
                key: column.key.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_saved_column_keys(layout: &DataTableLayout) -> Result<(), DataTableLayoutError> {
    let mut keys = HashSet::with_capacity(layout.columns.len());
    for column in &layout.columns {
        if !keys.insert(column.key.as_str()) {
            return Err(DataTableLayoutError::DuplicateSavedColumnKey {
                key: column.key.clone(),
            });
        }
    }
    Ok(())
}
