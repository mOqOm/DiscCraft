//! Graphic layers on menus (Encore's library 画像 / シェイプ placed on a menu): an image asset or a filled shape in
//! a rectangle, drawn over the background and under the texts and buttons.

use crate::ids::{AssetId, MenuId};
use crate::io::ProjectError;
use crate::menu::{DropShadow, Orientation, Rect, frame_size};
use crate::model::{AssetKind, Project};
use serde::{Deserialize, Serialize};

/// The most graphic layers on one menu.
pub const MAX_MENU_GRAPHICS: usize = 64;

/// Full opacity (per cent).
pub const FULL_OPACITY: u8 = 100;

/// A shape a graphic layer can be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShapeKind {
    /// A filled rectangle.
    Rectangle,
    /// A rectangle with rounded corners (the radius is an eighth of the shorter side).
    RoundedRectangle,
    /// An ellipse filling the rectangle.
    Ellipse,
}

impl ShapeKind {
    /// Every shape, in the library's order.
    pub const ALL: [ShapeKind; 3] = [
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle,
        ShapeKind::Ellipse,
    ];

    /// The name in command parameters.
    pub fn as_str(self) -> &'static str {
        match self {
            ShapeKind::Rectangle => "rectangle",
            ShapeKind::RoundedRectangle => "roundedRectangle",
            ShapeKind::Ellipse => "ellipse",
        }
    }

    /// Parses a command parameter.
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|shape| shape.as_str() == text)
    }
}

/// What a graphic layer shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GraphicKind {
    /// An image asset, scaled into the rectangle (transparent pixels stay transparent).
    Image {
        /// The image asset.
        asset: AssetId,
    },
    /// A shape filled with one colour.
    Shape {
        /// The shape.
        shape: ShapeKind,
        /// Fill colour.
        color: [u8; 3],
    },
}

fn full_opacity() -> u8 {
    FULL_OPACITY
}

fn is_full_opacity(opacity: &u8) -> bool {
    *opacity >= FULL_OPACITY
}

/// One graphic layer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuGraphic {
    /// Where it is drawn, in frame pixels.
    pub rect: Rect,
    /// What it shows.
    pub kind: GraphicKind,
    /// Opacity in per cent (1 to 100).
    #[serde(default = "full_opacity", skip_serializing_if = "is_full_opacity")]
    pub opacity: u8,
    /// A drop shadow behind the graphic (Encore's Object ▸ Drop Shadow…).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shadow: Option<DropShadow>,
    /// Mirrored or turned (Encore's Object ▸ Flip / Rotate); a turned image is drawn on its side.
    #[serde(default, skip_serializing_if = "Orientation::is_identity")]
    pub orientation: Orientation,
}

impl MenuGraphic {
    /// A short name for layer lists ("Image: sunset.png" / "Rectangle").
    pub fn kind_name(&self, project: &Project) -> String {
        match &self.kind {
            GraphicKind::Image { asset } => project
                .assets
                .get(asset)
                .map_or_else(|| "Image".to_owned(), |asset| asset.name.clone()),
            GraphicKind::Shape { shape, .. } => match shape {
                ShapeKind::Rectangle => "Rectangle".to_owned(),
                ShapeKind::RoundedRectangle => "Rounded Rectangle".to_owned(),
                ShapeKind::Ellipse => "Ellipse".to_owned(),
            },
        }
    }
}

/// Checks a rectangle against the frame: at least one pixel, inside the picture.
pub fn checked_graphic_rect(rect: Rect, frame: (u16, u16)) -> Result<Rect, ProjectError> {
    let (width, height) = frame;
    let right = rect.x.checked_add(rect.w);
    let bottom = rect.y.checked_add(rect.h);
    let fits = rect.w >= 1
        && rect.h >= 1
        && right.is_some_and(|right| right <= width)
        && bottom.is_some_and(|bottom| bottom <= height);
    if !fits {
        return Err(ProjectError::Invalid(format!(
            "the graphic must lie inside the {width} x {height} frame"
        )));
    }
    Ok(rect)
}

impl Project {
    /// Adds a graphic layer on top of the menu's other graphics; returns its index. An image must be an image
    /// asset of the project.
    pub fn add_menu_graphic(
        &mut self,
        menu: MenuId,
        graphic: MenuGraphic,
    ) -> Result<usize, ProjectError> {
        let frame = frame_size(&self.settings);
        let rect = checked_graphic_rect(graphic.rect, frame)?;
        if let GraphicKind::Image { asset } = graphic.kind {
            self.ensure_image_asset(asset)?;
        }
        let opacity = graphic.opacity.clamp(1, FULL_OPACITY);
        let target = self
            .menus
            .get_mut(&menu)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?;
        if target.graphics.len() >= MAX_MENU_GRAPHICS {
            return Err(ProjectError::Invalid(format!(
                "a menu holds at most {MAX_MENU_GRAPHICS} graphics"
            )));
        }
        target.graphics.push(MenuGraphic {
            rect,
            kind: graphic.kind,
            opacity,
            shadow: graphic.shadow.filter(DropShadow::is_valid),
            orientation: graphic.orientation.rotated(0),
        });
        Ok(target.graphics.len().saturating_sub(1))
    }

    /// Moves or resizes graphic `index`.
    pub fn set_menu_graphic_rect(
        &mut self,
        menu: MenuId,
        index: usize,
        rect: Rect,
    ) -> Result<(), ProjectError> {
        let frame = frame_size(&self.settings);
        let rect = checked_graphic_rect(rect, frame)?;
        self.menu_graphic_mut(menu, index)?.rect = rect;
        Ok(())
    }

    /// Changes what graphic `index` shows (an image must be an image asset of the project).
    pub fn set_menu_graphic_kind(
        &mut self,
        menu: MenuId,
        index: usize,
        kind: GraphicKind,
    ) -> Result<(), ProjectError> {
        if let GraphicKind::Image { asset } = kind {
            self.ensure_image_asset(asset)?;
        }
        self.menu_graphic_mut(menu, index)?.kind = kind;
        Ok(())
    }

    /// Sets the opacity of graphic `index` (1 to 100 per cent).
    pub fn set_menu_graphic_opacity(
        &mut self,
        menu: MenuId,
        index: usize,
        opacity: u8,
    ) -> Result<(), ProjectError> {
        if !(1..=FULL_OPACITY).contains(&opacity) {
            return Err(ProjectError::Invalid(
                "opacity must be 1 to 100".to_string(),
            ));
        }
        self.menu_graphic_mut(menu, index)?.opacity = opacity;
        Ok(())
    }

    /// Removes graphic `index`.
    pub fn remove_menu_graphic(&mut self, menu: MenuId, index: usize) -> Result<(), ProjectError> {
        let target = self
            .menus
            .get_mut(&menu)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?;
        if index >= target.graphics.len() {
            return Err(ProjectError::Invalid(
                "graphic index out of range".to_string(),
            ));
        }
        let _ = target.graphics.remove(index);
        Ok(())
    }

    /// Moves graphic `from` in the stacking order so that it sits at `to` (0 is the bottom).
    pub fn move_menu_graphic(
        &mut self,
        menu: MenuId,
        from: usize,
        to: usize,
    ) -> Result<(), ProjectError> {
        let target = self
            .menus
            .get_mut(&menu)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?;
        if from >= target.graphics.len() || to >= target.graphics.len() {
            return Err(ProjectError::Invalid(
                "graphic index out of range".to_string(),
            ));
        }
        let graphic = target.graphics.remove(from);
        target.graphics.insert(to, graphic);
        Ok(())
    }

    fn menu_graphic_mut(
        &mut self,
        menu: MenuId,
        index: usize,
    ) -> Result<&mut MenuGraphic, ProjectError> {
        self.menus
            .get_mut(&menu)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?
            .graphics
            .get_mut(index)
            .ok_or_else(|| ProjectError::Invalid("graphic index out of range".to_string()))
    }

    fn ensure_image_asset(&self, asset: AssetId) -> Result<(), ProjectError> {
        let is_image = self
            .assets
            .get(&asset)
            .is_some_and(|asset| asset.kind == AssetKind::Image);
        if !is_image {
            return Err(ProjectError::Invalid(
                "the asset is missing or not an image".to_string(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{FULL_OPACITY, GraphicKind, MAX_MENU_GRAPHICS, MenuGraphic, ShapeKind};
    use crate::menu::Rect;
    use crate::model::Project;
    use std::error::Error;
    use std::path::Path;

    fn shape(x: u16, w: u16) -> MenuGraphic {
        MenuGraphic {
            rect: Rect { x, y: 10, w, h: 20 },
            kind: GraphicKind::Shape {
                shape: ShapeKind::Ellipse,
                color: [200, 40, 40],
            },
            opacity: FULL_OPACITY,
            shadow: None,
            orientation: Default::default(),
        }
    }

    #[test]
    fn graphics_are_added_changed_moved_and_removed() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        let image = project.add_asset(Path::new("logo.png"))?;
        let video = project.add_asset(Path::new("movie.mp4"))?;

        assert_eq!(project.add_menu_graphic(menu, shape(0, 100))?, 0);
        assert_eq!(
            project.add_menu_graphic(
                menu,
                MenuGraphic {
                    rect: Rect {
                        x: 600,
                        y: 400,
                        w: 120,
                        h: 80
                    },
                    kind: GraphicKind::Image { asset: image },
                    opacity: 150,
                    shadow: None,
                    orientation: Default::default(),
                }
            )?,
            1
        );
        // Opacity is clamped to 100; a rectangle past the frame edge, an empty one and a non-image asset are refused.
        assert_eq!(
            project
                .menu(menu)
                .and_then(|m| m.graphics.get(1))
                .map(|g| g.opacity),
            Some(FULL_OPACITY)
        );
        assert!(project.add_menu_graphic(menu, shape(700, 100)).is_err());
        assert!(project.add_menu_graphic(menu, shape(0, 0)).is_err());
        assert!(
            project
                .add_menu_graphic(
                    menu,
                    MenuGraphic {
                        kind: GraphicKind::Image { asset: video },
                        ..shape(0, 10)
                    }
                )
                .is_err()
        );

        project.set_menu_graphic_rect(
            menu,
            0,
            Rect {
                x: 1,
                y: 2,
                w: 3,
                h: 4,
            },
        )?;
        assert!(
            project
                .set_menu_graphic_rect(
                    menu,
                    0,
                    Rect {
                        x: 0,
                        y: 470,
                        w: 10,
                        h: 20
                    }
                )
                .is_err()
        );
        project.set_menu_graphic_opacity(menu, 0, 50)?;
        assert!(project.set_menu_graphic_opacity(menu, 0, 0).is_err());
        project.set_menu_graphic_kind(
            menu,
            0,
            GraphicKind::Shape {
                shape: ShapeKind::Rectangle,
                color: [1, 2, 3],
            },
        )?;
        project.move_menu_graphic(menu, 0, 1)?;
        let kinds: Vec<GraphicKind> = project
            .menu(menu)
            .map(|m| m.graphics.iter().map(|g| g.kind).collect())
            .unwrap_or_default();
        assert!(matches!(kinds.first(), Some(GraphicKind::Image { .. })));
        assert!(matches!(kinds.get(1), Some(GraphicKind::Shape { .. })));

        project.remove_menu_graphic(menu, 1)?;
        assert!(project.remove_menu_graphic(menu, 1).is_err());
        assert_eq!(project.menu(menu).map(|m| m.graphics.len()), Some(1));

        // The image is in use, so the asset cannot be removed.
        assert!(project.remove_asset(image).is_err());
        Ok(())
    }

    #[test]
    fn the_graphic_count_is_capped() -> Result<(), Box<dyn Error>> {
        let mut project = Project::default();
        let menu = project.add_menu(None)?;
        for _ in 0..MAX_MENU_GRAPHICS {
            let _ = project.add_menu_graphic(menu, shape(0, 10))?;
        }
        assert!(project.add_menu_graphic(menu, shape(0, 10)).is_err());
        Ok(())
    }

    #[test]
    fn shapes_round_trip_their_names() {
        for shape in ShapeKind::ALL {
            assert_eq!(ShapeKind::parse(shape.as_str()), Some(shape));
        }
        assert_eq!(ShapeKind::parse("star"), None);
    }
}
