use serde::{Deserialize, Serialize};
use strum_macros::EnumIter;

use super::{data::PanelOrder, position::PanelPosition};
use crate::config::icon::LapceIcons;

#[derive(
    Clone, Copy, PartialEq, Serialize, Deserialize, Hash, Eq, Debug, EnumIter,
)]
pub enum PanelKind {
    Terminal,
    FileExplorer,
    SourceControl,
    GitLog,
    Plugin,
    SdkManager,
    Search,
    Problem,
    Debug,
    CallHierarchy,
    DocumentSymbol,
    References,
    Implementation,
}

impl PanelKind {
    pub fn svg_name(&self) -> &'static str {
        match &self {
            PanelKind::Terminal => LapceIcons::TERMINAL,
            PanelKind::FileExplorer => LapceIcons::FILE_EXPLORER,
            PanelKind::SourceControl => LapceIcons::SCM,
            PanelKind::GitLog => LapceIcons::GIT_LOG,
            PanelKind::Plugin => LapceIcons::EXTENSIONS,
            PanelKind::SdkManager => LapceIcons::SDK,
            PanelKind::Search => LapceIcons::SEARCH,
            PanelKind::Problem => LapceIcons::PROBLEM,
            PanelKind::Debug => LapceIcons::DEBUG,
            PanelKind::CallHierarchy => LapceIcons::TYPE_HIERARCHY,
            PanelKind::DocumentSymbol => LapceIcons::DOCUMENT_SYMBOL,
            PanelKind::References => LapceIcons::REFERENCES,
            PanelKind::Implementation => LapceIcons::IMPLEMENTATION,
        }
    }

    pub fn position(&self, order: &PanelOrder) -> Option<(usize, PanelPosition)> {
        for (pos, panels) in order.iter() {
            let index = panels.iter().position(|k| k == self);
            if let Some(index) = index {
                return Some((index, *pos));
            }
        }
        None
    }

    pub fn default_position(&self) -> PanelPosition {
        match self {
            PanelKind::Terminal => PanelPosition::BottomLeft,  // Terminal opens at bottom
            PanelKind::FileExplorer => PanelPosition::LeftTop,
            PanelKind::SourceControl => PanelPosition::LeftTop,
            PanelKind::GitLog => PanelPosition::BottomLeft,  // Git Log opens at bottom
            PanelKind::Plugin => PanelPosition::LeftTop,
            PanelKind::SdkManager => PanelPosition::LeftTop,
            PanelKind::Search => PanelPosition::LeftTop,
            PanelKind::Problem => PanelPosition::BottomLeft,
            PanelKind::Debug => PanelPosition::BottomLeft,
            PanelKind::CallHierarchy => PanelPosition::BottomLeft,
            PanelKind::DocumentSymbol => PanelPosition::RightTop,
            PanelKind::References => PanelPosition::BottomLeft,
            PanelKind::Implementation => PanelPosition::BottomLeft,
        }
    }
}
