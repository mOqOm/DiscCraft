use egui::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use egui::{Context, FontData, FontFamily};

/// 画面の文字に使うフォントの候補（ファイル、TTC の中の番号）。先に見つかったものを使う。
///
/// UI 向けの書体（Meiryo UI、Yu Gothic UI）を先に置く。egui はヒンティングをしないので、小さい文字でも形が
/// 崩れにくい Meiryo UI をいちばん先にする。
const FONT_CANDIDATES: &[(&str, u32)] = &[
    (r"C:\Windows\Fonts\meiryo.ttc", 2),  // Meiryo UI
    (r"C:\Windows\Fonts\YuGothM.ttc", 1), // Yu Gothic UI
    (r"C:\Windows\Fonts\YuGothR.ttc", 0), // Yu Gothic
    (r"C:\Windows\Fonts\msgothic.ttc", 0),
    ("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc", 0),
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0),
];

/// 日本語の書体を入れる。
///
/// 文章用（Proportional）ではいちばん先に置き、英数字も同じ書体で描く。英数字だけ egui の内蔵フォントになると、
/// 「本編A」のように和文と欧文の基準線がずれて読みにくい。等幅（タイムコードなど）は内蔵の等幅フォントを先に残し、
/// 日本語だけこの書体で補う。
pub fn install_japanese_font(ctx: &Context) {
    for (path, index) in FONT_CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if bytes.is_empty() {
            continue;
        }

        let mut data = FontData::from_owned(bytes);
        data.index = *index;
        let insert = FontInsert::new(
            "jp",
            data,
            vec![
                InsertFontFamily {
                    family: FontFamily::Proportional,
                    priority: FontPriority::Highest,
                },
                InsertFontFamily {
                    family: FontFamily::Monospace,
                    priority: FontPriority::Lowest,
                },
            ],
        );

        ctx.add_font(insert);
        ctx.request_repaint();
        return;
    }
}
