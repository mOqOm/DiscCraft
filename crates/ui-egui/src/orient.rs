//! Object ▸ Flip Horizontal / Flip Vertical / Rotate ▸ for the selected text, graphic or buttons.

use crate::app::DiscCraftApp;
use crate::i18n::tl;
use crate::shadow_dialog::ShadowTarget;
use serde_json::{Value, json};

/// One flip or turn, as the `menu.orient` parameter that does it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OrientChange {
    FlipHorizontal,
    FlipVertical,
    Rotate180,
    RotateClockwise,
    RotateCounterClockwise,
}

impl OrientChange {
    /// The part of the `menu.orient` parameters that says what to do.
    pub(crate) fn params(self) -> (&'static str, Value) {
        match self {
            OrientChange::FlipHorizontal => ("flip", json!("horizontal")),
            OrientChange::FlipVertical => ("flip", json!("vertical")),
            OrientChange::Rotate180 => ("rotate", json!(180)),
            OrientChange::RotateClockwise => ("rotate", json!(90)),
            OrientChange::RotateCounterClockwise => ("rotate", json!(270)),
        }
    }
}

/// The `menu.orient` parameters for `change` on `target`.
pub(crate) fn orient_params(target: &ShadowTarget, change: OrientChange) -> Value {
    let (key, value) = change.params();
    let mut params = json!({ "menu": target.menu().0, key: value });
    let (list, indexes) = match target {
        ShadowTarget::Text(_, index) => ("texts", vec![*index]),
        ShadowTarget::Graphic(_, index) => ("graphics", vec![*index]),
        ShadowTarget::Buttons(_, buttons) => ("buttons", buttons.clone()),
    };
    if let Some(object) = params.as_object_mut() {
        let _ = object.insert(list.to_owned(), json!(indexes));
    }
    params
}

impl DiscCraftApp {
    /// Flips or turns the selection (one undo step for all the selected buttons).
    pub(crate) fn orient_selected(&mut self, change: OrientChange) {
        let Some(target) = self.shadow_dialog_target() else {
            self.status = tl!("Select a text, button, image or shape first").to_string();
            return;
        };
        let _ = self.run_command("menu.orient", orient_params(&target, change));
    }
}

#[cfg(test)]
mod tests {
    use super::{OrientChange, orient_params};
    use crate::shadow_dialog::ShadowTarget;
    use disccraft_project::MenuId;
    use serde_json::json;

    #[test]
    fn the_parameters_name_the_objects_and_the_change() {
        assert_eq!(
            orient_params(
                &ShadowTarget::Buttons(MenuId(3), vec![0, 2]),
                OrientChange::RotateCounterClockwise
            ),
            json!({"menu": 3, "buttons": [0, 2], "rotate": 270})
        );
        assert_eq!(
            orient_params(
                &ShadowTarget::Text(MenuId(1), 4),
                OrientChange::FlipVertical
            ),
            json!({"menu": 1, "texts": [4], "flip": "vertical"})
        );
    }
}
