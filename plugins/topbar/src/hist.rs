//! Win+V without Explorer (#171): the bar listens to the clipboard and keeps the last texts copied; Win+V lists them in a menu
//! and the picked one is put back on the clipboard and pasted into the window that had the focus. Text only, in memory only:
//! nothing is written to disk, and copies a password manager marks as private are not kept.
use std::cell::RefCell;
use windows_sys::Win32::{
    Foundation::*,
    System::{DataExchange::*, Memory::*},
    UI::{Input::KeyboardAndMouse::*, WindowsAndMessaging::*},
};

const MAX: usize = 25;
const CF_UNICODETEXT: u32 = 13;

thread_local! {
    static HIST: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Starts listening (WM_CLIPBOARDUPDATE comes to `bar`).
pub unsafe fn init(bar: HWND) {
    AddClipboardFormatListener(bar);
}

/// The newest copy goes on top; a repeat moves up instead of appearing twice.
pub fn push(v: &mut Vec<String>, s: String) {
    if s.trim().is_empty() {
        return;
    }
    v.retain(|x| *x != s);
    v.insert(0, s);
    v.truncate(MAX);
}

fn format(name: &str) -> u32 {
    let w: Vec<u16> = name.encode_utf16().chain([0]).collect();
    unsafe { RegisterClipboardFormatW(w.as_ptr()) }
}

/// WM_CLIPBOARDUPDATE: keeps the new text, unless the copying app asked clipboard history not to keep it.
pub unsafe fn on_update(bar: HWND) {
    let private = ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "Clipboard Viewer Ignore"].iter().any(|f| IsClipboardFormatAvailable(format(f)) != 0);
    if private || IsClipboardFormatAvailable(CF_UNICODETEXT) == 0 || OpenClipboard(bar) == 0 {
        return;
    }
    let h = GetClipboardData(CF_UNICODETEXT);
    let mut s = None;
    if !h.is_null() {
        let p = GlobalLock(h as _) as *const u16;
        if !p.is_null() {
            let n = (0..GlobalSize(h as _) / 2).take_while(|&i| *p.add(i) != 0).count();
            s = Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)));
            GlobalUnlock(h as _);
        }
    }
    CloseClipboard();
    if let Some(s) = s {
        HIST.with_borrow_mut(|v| push(v, s));
    }
}

unsafe fn set_text(bar: HWND, s: &str) -> bool {
    let w: Vec<u16> = s.encode_utf16().chain([0]).collect();
    let g = GlobalAlloc(GMEM_MOVEABLE, w.len() * 2);
    if g.is_null() {
        return false;
    }
    let p = GlobalLock(g) as *mut u16;
    std::ptr::copy_nonoverlapping(w.as_ptr(), p, w.len());
    GlobalUnlock(g);
    if OpenClipboard(bar) == 0 {
        GlobalFree(g);
        return false;
    }
    EmptyClipboard();
    let ok = !SetClipboardData(CF_UNICODETEXT, g as _).is_null(); // the clipboard owns the memory from here on
    CloseClipboard();
    if !ok {
        GlobalFree(g);
    }
    ok
}

/// One line of the menu: the text on one line, cut to a readable length.
fn label(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 60 { format!("{}…", one.chars().take(60).collect::<String>()) } else { one }
}

/// Win+V: the history as a menu at the middle of the bar; the pick is pasted into the window that was in front.
pub unsafe fn show(bar: HWND, cx: i32) {
    let target = GetForegroundWindow();
    let v = HIST.with_borrow(|v| v.clone());
    if v.is_empty() {
        let items = vec![crate::fly::MItem::new(winrust_common::t!("Буфер обмена пуст: скопируйте текст", "The clipboard history is empty: copy some text", ja: "クリップボードの履歴は空です。テキストをコピーしてください", pt: "O histórico da área de transferência está vazio: copie um texto", zh: "剪贴板历史为空：请复制一些文本"))];
        return crate::fly::menu(cx, None, items, |_| {});
    }
    let mut items: Vec<crate::fly::MItem> = v.iter().map(|s| crate::fly::MItem::new(label(s))).collect();
    items.push(crate::fly::MItem::new(winrust_common::t!("Очистить всё", "Clear all", ja: "すべてクリア", pt: "Limpar tudo", zh: "全部清除")).sep());
    let bar = bar as usize;
    crate::fly::menu(cx, None, items, move |i| {
        let Some(s) = v.get(i) else {
            return HIST.with_borrow_mut(|h| h.clear());
        };
        if set_text(bar as HWND, s) && !target.is_null() {
            crate::force_fg(target);
            crate::keys(&[VK_CONTROL, b'V' as u16]);
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn history_keeps_newest_first_without_repeats() {
        let mut v = vec![];
        for s in ["a", "b", "a", " ", "c"] {
            super::push(&mut v, s.to_string());
        }
        assert_eq!(v, ["c", "a", "b"]);
        for i in 0..40 {
            super::push(&mut v, i.to_string());
        }
        assert_eq!((v.len(), v[0].as_str()), (25, "39"));
        assert_eq!(super::label("x\n  y\tz"), "x y z");
        assert!(super::label(&"й".repeat(100)).ends_with('…'));
    }
}
