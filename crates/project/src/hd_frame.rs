//! Menus on an HD frame (a Blu-ray disc whose titles are HD, ROADMAP M10): the menus are drawn and edited at the
//! HD size, as Encore's Blu-ray menus are. A menu laid out for SD moves onto the HD frame when the disc becomes HD
//! (and back): a 4:3 menu keeps its picture in the middle of the 16:9 frame, a 16:9 (anamorphic) menu fills it.

use crate::menu::{DropShadow, Menu, Rect, frame_size};
use crate::model::{DiscSettings, Project};

/// Width of the SD frame.
const SD_W: f64 = 720.0;

/// Whether the menus of `settings` are on an HD frame (square pixels, 16:9).
pub fn hd_frame(settings: &DiscSettings) -> bool {
    frame_size(settings).0 > 720
}

/// A rectangle laid out for a 720 x 480 4:3 menu (the library's presets, DiscCraft's own layouts), on the frame
/// of `settings`: on SD x and the width stay and y and the height follow the frame's height; on HD the 4:3
/// picture sits in the middle of the 16:9 frame.
pub fn design_rect(settings: &DiscSettings, rect: Rect) -> Rect {
    let (frame_w, frame_h) = frame_size(settings);
    let (fw, fh) = (f64::from(frame_w), f64::from(frame_h));
    let sy = fh / 480.0;
    let (sx, left) = if hd_frame(settings) {
        let picture_w = fh * 4.0 / 3.0;
        (picture_w / SD_W, (fw - picture_w) / 2.0)
    } else {
        (1.0, 0.0)
    };
    let x = left + f64::from(rect.x) * sx;
    let y = f64::from(rect.y) * sy;
    clamp(
        (x, y, f64::from(rect.w) * sx, f64::from(rect.h) * sy),
        (frame_w, frame_h),
    )
}

/// Where one frame's content shows on a 16:9 picture (0 to 1 across, 0 to 1 down): `(left, width)` across.
fn span(frame: (u16, u16), wide: bool) -> (f64, f64) {
    if frame.0 > 720 || wide {
        (0.0, 1.0)
    } else {
        // A 4:3 SD frame fills the middle three quarters of a 16:9 picture.
        (0.125, 0.75)
    }
}

/// The scale that moves a menu from one frame to another: `(sx, dx, sy)` with x' = dx + x * sx and y' = y * sy.
fn mapping(from: (u16, u16), to: (u16, u16), wide: bool, wide_after: bool) -> (f64, f64, f64) {
    let (from_left, from_w) = span(from, wide);
    let (to_left, to_w) = span(to, wide_after);
    let (fw, tw) = (f64::from(from.0.max(1)), f64::from(to.0.max(1)));
    // x / fw * from_w + from_left = (x' / tw) * to_w + to_left
    let sx = from_w / fw * tw / to_w;
    let dx = (from_left - to_left) / to_w * tw;
    let sy = f64::from(to.1) / f64::from(from.1.max(1));
    (sx, dx, sy)
}

fn clamp((x, y, w, h): (f64, f64, f64, f64), (frame_w, frame_h): (u16, u16)) -> Rect {
    let to_u16 = |value: f64, max: u16| -> u16 {
        // Inside 0..=max, so the cast is exact.
        value.round().clamp(0.0, f64::from(max)) as u16
    };
    let x = to_u16(x, frame_w.saturating_sub(1));
    let y = to_u16(y, frame_h.saturating_sub(1));
    Rect {
        x,
        y,
        w: to_u16(w, frame_w.saturating_sub(x)).max(1),
        h: to_u16(h, frame_h.saturating_sub(y)).max(1),
    }
}

fn scale_shadow(shadow: &mut Option<DropShadow>, sy: f64) {
    if let Some(shadow) = shadow {
        let scale = |value: u8| (f64::from(value) * sy).round().clamp(0.0, 255.0) as u8;
        shadow.distance = scale(shadow.distance);
        shadow.size = scale(shadow.size);
    }
}

/// Moves `menu` from a frame of `from` to a frame of `to`.
fn retarget_menu(menu: &mut Menu, from: (u16, u16), to: (u16, u16)) {
    let wide = menu.widescreen;
    // On HD the frame itself is 16:9 (square pixels); back on SD the menu is a 16:9 one.
    let wide_after = if to.0 > 720 {
        false
    } else {
        wide || from.0 > 720
    };
    let (sx, dx, sy) = mapping(from, to, wide, wide_after);
    let move_rect = |rect: Rect| {
        clamp(
            (
                dx + f64::from(rect.x) * sx,
                f64::from(rect.y) * sy,
                f64::from(rect.w) * sx,
                f64::from(rect.h) * sy,
            ),
            to,
        )
    };
    for button in &mut menu.buttons {
        button.rect = move_rect(button.rect);
        scale_shadow(&mut button.shadow, sy);
    }
    for text in &mut menu.texts {
        let at = move_rect(Rect {
            x: text.x,
            y: text.y,
            w: 1,
            h: 1,
        });
        text.x = at.x;
        text.y = at.y;
        text.size = (f64::from(text.size) * sy)
            .round()
            .clamp(1.0, f64::from(u16::MAX)) as u16;
        scale_shadow(&mut text.shadow, sy);
    }
    for graphic in &mut menu.graphics {
        graphic.rect = move_rect(graphic.rect);
        scale_shadow(&mut graphic.shadow, sy);
    }
    for guide in &mut menu.guides {
        let position = f64::from(guide.position);
        guide.position = match guide.orientation {
            crate::menu::GuideOrientation::Horizontal => position * sy,
            crate::menu::GuideOrientation::Vertical => dx + position * sx,
        }
        .round()
        .clamp(0.0, f64::from(u16::MAX)) as u16;
    }
    menu.widescreen = wide_after;
}

impl Project {
    /// After the disc's settings changed from `before`, moves every menu onto the new frame when it went to or
    /// from HD (an SD-to-SD change, such as NTSC to PAL, leaves the menus as they are).
    pub fn retarget_menus(&mut self, before: &DiscSettings) {
        let (from, to) = (frame_size(before), frame_size(&self.settings));
        if from == to || !(from.0 > 720 || to.0 > 720) {
            return;
        }
        for menu in self.menus.values_mut() {
            retarget_menu(menu, from, to);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{design_rect, hd_frame};
    use crate::model::{BdVideo, DiscKind, DiscSettings, Project};
    use crate::{Link, Rect};

    fn hd() -> DiscSettings {
        DiscSettings {
            kind: DiscKind::BluRay,
            bd_video: BdVideo::Hd1080p23,
            ..DiscSettings::default()
        }
    }

    #[test]
    fn an_sd_layout_sits_in_the_middle_of_an_hd_frame() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 720,
            h: 480,
        };
        assert!(!hd_frame(&DiscSettings::default()));
        assert!(hd_frame(&hd()));
        assert_eq!(design_rect(&DiscSettings::default(), rect), rect);
        assert_eq!(
            design_rect(&hd(), rect),
            Rect {
                x: 240,
                y: 0,
                w: 1440,
                h: 1080
            }
        );
        // On a DVD-sized Blu-ray (SD), nothing moves even when the HD video is chosen for a DVD project.
        let dvd = DiscSettings {
            bd_video: BdVideo::Hd1080p23,
            ..DiscSettings::default()
        };
        assert!(!hd_frame(&dvd));
    }

    #[test]
    fn menus_move_to_hd_and_back() -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::default();
        project.settings.kind = DiscKind::BluRay;
        let menu = project.add_menu(None)?;
        let rect = Rect {
            x: 100,
            y: 200,
            w: 200,
            h: 40,
        };
        let _ = project.add_button(menu, "Play".to_owned(), Some(rect), Link::Stop)?;
        let wide = project.add_menu(None)?;
        let _ = project.add_button(wide, "Play".to_owned(), Some(rect), Link::Stop)?;
        if let Some(found) = project.menu_mut(wide) {
            found.widescreen = true;
        }

        let before = project.settings.clone();
        project.settings.bd_video = BdVideo::Hd1080p23;
        project.retarget_menus(&before);
        let moved = |project: &Project, id| {
            project
                .menu(id)
                .and_then(|m| m.buttons.first())
                .map(|b| b.rect)
        };
        // 4:3: x 240 + 2x, y 2.25y.
        assert_eq!(
            moved(&project, menu),
            Some(Rect {
                x: 440,
                y: 450,
                w: 400,
                h: 90
            })
        );
        // 16:9 SD: x 8/3 x.
        assert_eq!(
            moved(&project, wide),
            Some(Rect {
                x: 267,
                y: 450,
                w: 533,
                h: 90
            })
        );
        assert!(project.menu(menu).is_some_and(|m| !m.widescreen));

        // Back to SD: the 16:9 menu is where it was; the 4:3 one is the same picture as a 16:9 menu.
        let before = project.settings.clone();
        project.settings.bd_video = BdVideo::Sd;
        project.retarget_menus(&before);
        assert_eq!(moved(&project, wide), Some(rect));
        assert!(project.menu(wide).is_some_and(|m| m.widescreen));
        assert_eq!(
            moved(&project, menu),
            Some(Rect {
                x: 165,
                y: 200,
                w: 150,
                h: 40
            })
        );
        Ok(())
    }
}
