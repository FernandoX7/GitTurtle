//! Persisted history columns. Narrow windows scroll instead of hiding choices.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnId {
    Refs,
    Graph,
    Subject,
    Author,
    Date,
    Sha,
}

impl ColumnId {
    pub const ALL: [Self; 6] = [
        Self::Refs,
        Self::Graph,
        Self::Subject,
        Self::Author,
        Self::Date,
        Self::Sha,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Refs => "References",
            Self::Graph => "Graph",
            Self::Subject => "Commit message",
            Self::Author => "Author",
            Self::Date => "Date",
            Self::Sha => "SHA",
        }
    }

    pub fn width_bounds(self) -> (f32, f32) {
        match self {
            Self::Refs => (80., 360.),
            Self::Graph => (64., 480.),
            Self::Subject => (180., 1600.),
            Self::Author => (80., 400.),
            Self::Date => (64., 200.),
            Self::Sha => (64., 240.),
        }
    }

    pub fn default_width(self) -> f32 {
        match self {
            Self::Refs => 140.,
            Self::Graph => 112.,
            Self::Subject => 320.,
            Self::Author => 136.,
            Self::Date => 92.,
            Self::Sha => 84.,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnSetting {
    pub visible: bool,
    pub width: f32,
}

impl Default for ColumnSetting {
    fn default() -> Self {
        Self {
            visible: true,
            // Normalization supplies the correct default for each column.
            width: 0.,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ColumnSettings {
    pub refs: ColumnSetting,
    pub graph: ColumnSetting,
    pub subject: ColumnSetting,
    pub author: ColumnSetting,
    pub date: ColumnSetting,
    pub sha: ColumnSetting,
}

impl Default for ColumnSettings {
    fn default() -> Self {
        let column = |id: ColumnId| ColumnSetting {
            visible: true,
            width: id.default_width(),
        };
        Self {
            refs: column(ColumnId::Refs),
            graph: column(ColumnId::Graph),
            subject: column(ColumnId::Subject),
            author: column(ColumnId::Author),
            date: column(ColumnId::Date),
            sha: column(ColumnId::Sha),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibleColumn {
    pub id: ColumnId,
    pub width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColumnLayout {
    pub columns: Vec<VisibleColumn>,
    pub content_width: f32,
}

impl ColumnSettings {
    pub fn get(&self, id: ColumnId) -> &ColumnSetting {
        match id {
            ColumnId::Refs => &self.refs,
            ColumnId::Graph => &self.graph,
            ColumnId::Subject => &self.subject,
            ColumnId::Author => &self.author,
            ColumnId::Date => &self.date,
            ColumnId::Sha => &self.sha,
        }
    }

    pub fn get_mut(&mut self, id: ColumnId) -> &mut ColumnSetting {
        match id {
            ColumnId::Refs => &mut self.refs,
            ColumnId::Graph => &mut self.graph,
            ColumnId::Subject => &mut self.subject,
            ColumnId::Author => &mut self.author,
            ColumnId::Date => &mut self.date,
            ColumnId::Sha => &mut self.sha,
        }
    }

    pub fn normalize(&mut self) {
        for id in ColumnId::ALL {
            let column = self.get_mut(id);
            let (min, max) = id.width_bounds();
            column.width = if column.width.is_finite() && column.width > 0. {
                column.width.clamp(min, max)
            } else {
                id.default_width()
            };
        }
        self.subject.visible = true;
    }

    pub fn set_visible(&mut self, id: ColumnId, visible: bool) {
        self.get_mut(id).visible = visible || id == ColumnId::Subject;
    }

    pub fn set_width(&mut self, id: ColumnId, width: f32) {
        self.get_mut(id).width = width;
        self.normalize();
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The subject absorbs spare width; visible columns never disappear merely
    /// because the viewport is small. Render header and rows from this same layout.
    #[cfg(test)]
    pub fn layout(&self, available_width: f32) -> ColumnLayout {
        self.layout_for_graph(available_width, 0.)
    }

    /// Reserve a useful graph viewport without allowing ancestry far below the
    /// current viewport to push commit messages offscreen. Wider graphs scroll
    /// within this column at their original lane spacing; a deliberate saved
    /// width can still exceed the automatic allowance.
    pub fn layout_for_graph(&self, available_width: f32, graph_minimum: f32) -> ColumnLayout {
        let mut settings = self.clone();
        settings.normalize();
        let graph_allowance = if available_width.is_finite() {
            (available_width * 0.25).clamp(112., 280.)
        } else {
            112.
        };
        let mut columns: Vec<_> = ColumnId::ALL
            .into_iter()
            .filter_map(|id| {
                let column = settings.get(id);
                column.visible.then_some(VisibleColumn {
                    id,
                    width: if id == ColumnId::Graph && graph_minimum.is_finite() {
                        column.width.max(graph_minimum.clamp(0., graph_allowance))
                    } else {
                        column.width
                    },
                })
            })
            .collect();
        let minimum_width: f32 = columns.iter().map(|column| column.width).sum();
        let available_width = if available_width.is_finite() {
            available_width.max(0.)
        } else {
            0.
        };
        let extra = (available_width - minimum_width).max(0.);
        if let Some(subject) = columns
            .iter_mut()
            .find(|column| column.id == ColumnId::Subject)
        {
            subject.width += extra;
        }
        ColumnLayout {
            columns,
            content_width: minimum_width + extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_graph_lanes_do_not_push_messages_out_of_the_viewport() {
        let settings = ColumnSettings::default();
        let old = settings.clone();
        let layout = settings.layout_for_graph(500., 900.);
        assert_eq!(
            layout
                .columns
                .iter()
                .find(|c| c.id == ColumnId::Graph)
                .unwrap()
                .width,
            125.
        );
        assert!(layout.content_width < 900.);
        assert_eq!(settings, old);
        let again = settings.layout_for_graph(500., 80.);
        assert_eq!(
            again
                .columns
                .iter()
                .find(|c| c.id == ColumnId::Graph)
                .unwrap()
                .width,
            settings.graph.width
        );
        let mut custom = settings;
        custom.set_width(ColumnId::Graph, 400.);
        assert_eq!(custom.layout_for_graph(500., 2000.).columns[1].width, 400.);
    }

    #[test]
    fn narrow_layout_preserves_all_chosen_columns_and_wide_layout_fills_subject() {
        let settings = ColumnSettings::default();
        let narrow = settings.layout(350.);
        assert_eq!(narrow.columns.len(), 6);
        assert!(narrow.content_width > 350.);
        let wide = settings.layout(narrow.content_width + 200.);
        assert_eq!(wide.content_width, narrow.content_width + 200.);
        for (left, right) in narrow.columns.iter().zip(&wide.columns) {
            assert_eq!(left.id, right.id);
            assert_eq!(
                right.width - left.width,
                if left.id == ColumnId::Subject {
                    200.
                } else {
                    0.
                }
            );
        }
        assert_eq!(settings, ColumnSettings::default());
    }

    #[test]
    fn preferences_keep_subject_available_bound_widths_and_reset() {
        let mut settings = ColumnSettings::default();
        settings.set_visible(ColumnId::Author, false);
        settings.set_visible(ColumnId::Subject, false);
        settings.set_width(ColumnId::Graph, f32::NAN);
        settings.set_width(ColumnId::Refs, f32::MAX);
        settings.set_width(ColumnId::Date, 1.);
        assert!(settings.subject.visible);
        assert!(!settings.author.visible);
        assert_eq!(settings.graph.width, ColumnId::Graph.default_width());
        assert_eq!(settings.refs.width, ColumnId::Refs.width_bounds().1);
        assert_eq!(settings.date.width, ColumnId::Date.width_bounds().0);
        assert_eq!(settings.layout(10.).columns.len(), 5);
        assert!(settings.layout(f32::INFINITY).content_width.is_finite());
        settings.reset();
        assert_eq!(settings, ColumnSettings::default());
    }

    #[test]
    fn partial_column_settings_use_per_column_defaults_and_round_trip() {
        let mut settings: ColumnSettings = serde_json::from_str(
            r#"{"refs":{"visible":false},"subject":{"visible":false,"width":-1},"sha":{"width":120}}"#,
        ).unwrap();
        settings.normalize();
        assert_eq!(settings.refs.width, ColumnId::Refs.default_width());
        assert!(!settings.refs.visible);
        assert!(settings.subject.visible);
        assert_eq!(settings.subject.width, ColumnId::Subject.default_width());
        assert_eq!(settings.sha.width, 120.);
        let encoded = serde_json::to_vec(&settings).unwrap();
        assert_eq!(
            serde_json::from_slice::<ColumnSettings>(&encoded).unwrap(),
            settings
        );
    }
}
