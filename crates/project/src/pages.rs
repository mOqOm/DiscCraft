//! Encore's menu pages (メニュー ▸ 新規ページを追加): a menu can have later pages, each an empty menu of its own
//! with the first page's look and settings. A page is a menu of the project (the build makes one menu PGC per
//! page, and links reach a page like any menu); [`crate::Menu::page_of`] ties it to its first page, which is the
//! one the project lists show.

use crate::ids::MenuId;
use crate::io::ProjectError;
use crate::menu::Menu;
use crate::model::Project;

impl Project {
    /// The first page of `menu`: itself, unless it is a later page.
    pub fn menu_root(&self, menu: MenuId) -> MenuId {
        self.menu(menu)
            .and_then(|found| found.page_of)
            .filter(|root| self.menus.contains_key(root))
            .unwrap_or(menu)
    }

    /// Whether `menu` is a later page of another menu (the lists show only first pages).
    pub fn is_menu_page(&self, menu: MenuId) -> bool {
        self.menu_root(menu) != menu
    }

    /// The pages of the menu `menu` belongs to, the first page first.
    pub fn menu_pages(&self, menu: MenuId) -> Vec<MenuId> {
        let root = self.menu_root(menu);
        std::iter::once(root)
            .chain(
                self.menus
                    .iter()
                    .filter(|(id, found)| **id != root && found.page_of == Some(root))
                    .map(|(id, _)| *id),
            )
            .collect()
    }

    /// The page number of `menu`, counted from 0 as Encore does (0 for the first page).
    pub fn menu_page_number(&self, menu: MenuId) -> usize {
        self.menu_pages(menu)
            .iter()
            .position(|page| *page == menu)
            .unwrap_or(0)
    }

    /// Adds a page to the menu `menu` belongs to (Encore's 新規ページを追加): an empty menu (no buttons, texts or
    /// pictures) with the first page's background and settings. Returns the new page.
    pub fn add_menu_page(&mut self, menu: MenuId) -> Result<MenuId, ProjectError> {
        let root = self.menu_root(menu);
        let first = self
            .menu(root)
            .cloned()
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?;
        let id = self.add_menu(Some(first.name.clone()))?;
        let page = self
            .menu_mut(id)
            .ok_or_else(|| ProjectError::Invalid("menu does not exist".to_string()))?;
        *page = Menu {
            id,
            buttons: Vec::new(),
            texts: Vec::new(),
            graphics: Vec::new(),
            default_button: 0,
            page_of: Some(root),
            ..first
        };
        Ok(id)
    }
}

#[cfg(test)]
mod tests {
    use crate::{Link, MenuBackground, Project};

    #[test]
    fn a_page_is_an_empty_menu_with_the_first_pages_look() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut project = Project::default();
        let main = project.add_menu(Some("Main".to_owned()))?;
        let _ = project.add_button(main, "Play".to_owned(), None, Link::Stop)?;
        if let Some(menu) = project.menu_mut(main) {
            menu.background = MenuBackground::Color { color: [0, 0, 80] };
            menu.end_action = Link::Stop;
        }
        let page = project.add_menu_page(main)?;
        let second = project.add_menu_page(page)?;

        let first = project.menu(page).ok_or("missing page")?;
        assert!(first.buttons.is_empty());
        assert_eq!(
            first.background,
            MenuBackground::Color { color: [0, 0, 80] }
        );
        assert_eq!(first.end_action, Link::Stop);
        assert_eq!(project.menu_pages(second), [main, page, second]);
        assert_eq!(project.menu_page_number(main), 0);
        assert_eq!(project.menu_page_number(second), 2);
        assert!(project.is_menu_page(page));
        assert!(!project.is_menu_page(main));

        // Removing the menu removes its pages.
        project.remove_menu(main)?;
        assert!(project.menus.is_empty());
        Ok(())
    }
}
