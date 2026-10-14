//! The top of a ticket's sub-issue chain: its EPIC, or whatever root it hangs
//! under. Reports group and tag lines by it, and Search filters on it, so both
//! share this one definition.

use std::collections::HashMap;

use crate::app::App;
use crate::models::item::Item;

/// Title on one line.
///
/// Titles come straight from GitHub and some carry an embedded newline or a
/// trailing space (a body pasted into the title field). Printed raw they broke
/// a report line in half, so the status and priority of a ticket ended up on
/// their own line in Slack.
pub fn clean_title(title: &str) -> String {
    title.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Whether an issue is an EPIC.
///
/// The board is not consistent about this: some epics carry the `Epic` label,
/// others only the `[Epic]` title prefix, so both signals count.
pub fn is_epic(item: &Item) -> bool {
    item.labels.iter().any(|l| App::label_eq(l, "Epic"))
        || clean_title(&item.title)
            .to_lowercase()
            .starts_with("[epic]")
}

/// Longest epic name a report line will carry.
const EPIC_NAME_MAX: usize = 32;

/// Depth cap for the parent walk - a cycle on the board would otherwise hang
/// the report.
const MAX_PARENT_HOPS: usize = 4;

/// Search filter value for "no parent at all".
pub const NO_PARENT: &str = "none";

/// Epic name without its `[Epic]` prefix, short enough for a report line.
pub fn epic_short_name(item: &Item) -> String {
    short_root_name(&item.title)
}

/// A root ticket's title without an `[Epic]` prefix, cut to fit a report line.
pub fn short_root_name(raw: &str) -> String {
    let title = clean_title(raw);
    let name = title
        .strip_prefix("[Epic]")
        .or_else(|| title.strip_prefix("[EPIC]"))
        .or_else(|| title.strip_prefix("[epic]"))
        .unwrap_or(title.as_str())
        .trim()
        .to_string();
    if name.chars().count() > EPIC_NAME_MAX {
        let cut: String = name.chars().take(EPIC_NAME_MAX).collect();
        format!("{}...", cut.trim_end())
    } else {
        name
    }
}

/// The top of a ticket's sub-issue chain.
///
/// Half of Sprint 29 had no epic above it, and "[no epic]" said nothing about
/// where those tickets belonged: 24 hung off `#3916 Issues sprint 29`, a
/// plain task the QA files bugs under. The root names that.
pub enum Root<'a> {
    /// An epic on the board.
    Epic(&'a Item),
    /// The chain ends at a ticket on the board that is not an epic.
    Ticket(&'a Item),
    /// The chain leaves the board: sync stores each parent's number and
    /// title, so the first parent that is not a project item can still be
    /// named. Parents are looked up in the child's repository, so that is
    /// the off-board parent's repository too.
    OffBoard {
        number: u32,
        title: Option<String>,
        repo: Option<String>,
    },
    /// No parent at all.
    None,
}

impl Root<'_> {
    /// Short name for a report line, `None` when there is no chain.
    pub fn short_name(&self) -> Option<String> {
        match self {
            Root::Epic(i) | Root::Ticket(i) => Some(epic_short_name(i)),
            Root::OffBoard {
                title: Some(title), ..
            } => Some(short_root_name(title)),
            Root::OffBoard {
                number,
                title: None,
                ..
            } => Some(format!("#{}", number)),
            Root::None => None,
        }
    }

    /// Name of the rollup group, with the format's own issue link.
    pub fn group_name(&self, link: &dyn Fn(&Item) -> String) -> String {
        match self {
            Root::Epic(i) | Root::Ticket(i) => format!("{} {}", link(i), epic_short_name(i)),
            Root::OffBoard { number, title, .. } => format!(
                "#{} {}",
                number,
                title
                    .as_deref()
                    .map(short_root_name)
                    .unwrap_or_else(|| "(not on the board)".to_string())
            ),
            Root::None => "No parent".to_string(),
        }
    }

    /// Grouping key. Board items by project item id (issue numbers repeat
    /// across the project's repositories), off-board parents by number.
    pub fn key(&self) -> String {
        match self {
            Root::Epic(i) | Root::Ticket(i) => format!("item:{}", i.id),
            Root::OffBoard { number, .. } => format!("off:{}", number),
            Root::None => "none".to_string(),
        }
    }

    /// Sort rank: epics, then other roots, then "no parent".
    pub fn rank(&self) -> u8 {
        match self {
            Root::Epic(_) => 0,
            Root::Ticket(_) | Root::OffBoard { .. } => 1,
            Root::None => 2,
        }
    }

    /// Search filter value: `owner/repo#number`, or [`NO_PARENT`].
    ///
    /// Stable across sessions, so a saved filter keeps pointing at the same
    /// root, and qualified by repository because issue numbers repeat across
    /// the project's repositories.
    pub fn filter_value(&self) -> String {
        match self {
            Root::Epic(i) | Root::Ticket(i) => format!(
                "{}#{}",
                i.repository.as_deref().unwrap_or(""),
                i.number.unwrap_or(0)
            ),
            Root::OffBoard { number, repo, .. } => {
                format!("{}#{}", repo.as_deref().unwrap_or(""), number)
            }
            Root::None => NO_PARENT.to_string(),
        }
    }

    /// Search filter label: `#2388 RBAC Data Visibility Controls...`.
    pub fn filter_label(&self) -> String {
        let number = match self {
            Root::Epic(i) | Root::Ticket(i) => i.number,
            Root::OffBoard { number, .. } => Some(*number),
            Root::None => return "No parent".to_string(),
        };
        match (number, self.short_name()) {
            (Some(n), Some(name)) if name != format!("#{}", n) => format!("#{} {}", n, name),
            (Some(n), _) => format!("#{}", n),
            (None, Some(name)) => name,
            (None, None) => "(draft)".to_string(),
        }
    }
}

/// Walk up the sub-issue chain to its root, finding each parent with `find`
/// (number, repository).
fn walk<'a>(item: &Item, find: impl Fn(u32, Option<&str>) -> Option<&'a Item>) -> Root<'a> {
    let Some(mut number) = item.parent_number else {
        return Root::None;
    };
    let mut title = item.parent_title.clone();
    let repo = item.repository.as_deref();
    for _ in 0..MAX_PARENT_HOPS {
        let Some(parent) = find(number, repo) else {
            return Root::OffBoard {
                number,
                title,
                repo: repo.map(str::to_string),
            };
        };
        if is_epic(parent) {
            return Root::Epic(parent);
        }
        match parent.parent_number {
            Some(next) => {
                number = next;
                title = parent.parent_title.clone();
            }
            None => return Root::Ticket(parent),
        }
    }
    // Deeper than any real chain: treat as a cycle and give up.
    Root::None
}

/// Walk up the sub-issue chain to its root.
///
/// Sync fetches only ONE parent level (`parent_number`, `parent_title`), so
/// the rest of the chain is rebuilt from the board itself: the parent is
/// looked up among the loaded items and its own parent read from there.
/// Most tickets hang straight off an epic, but some sit under a story that
/// hangs off one.
///
/// Matching on number AND repository is deliberate: the project spans several
/// repositories and the same issue number exists in more than one of them.
pub fn resolve_root<'a>(item: &Item, items: &'a [Item]) -> Root<'a> {
    walk(item, |number, repo| {
        items
            .iter()
            .find(|i| i.number == Some(number) && i.repository.as_deref() == repo)
    })
}

/// [`resolve_root`] for many items at once.
///
/// `resolve_root` scans the board for every parent hop, which is fine for a
/// report but not for filtering 2,600 tickets on every keystroke. This looks
/// parents up by (repository, number) instead.
pub struct RootIndex<'a> {
    items: &'a [Item],
    by_number: HashMap<(Option<&'a str>, u32), usize>,
}

impl<'a> RootIndex<'a> {
    pub fn new(items: &'a [Item]) -> Self {
        let mut by_number = HashMap::with_capacity(items.len());
        for (idx, item) in items.iter().enumerate() {
            if let Some(n) = item.number {
                // First wins, like `resolve_root`'s `find`.
                by_number
                    .entry((item.repository.as_deref(), n))
                    .or_insert(idx);
            }
        }
        RootIndex { items, by_number }
    }

    pub fn resolve(&self, item: &Item) -> Root<'a> {
        let items = self.items;
        walk(item, |number, repo| {
            self.by_number
                .get(&(repo, number))
                .and_then(|&idx| items.get(idx))
        })
    }
}

/// Options for the Search EPIC row as (filter value, label).
///
/// Only roots with open work are offered - a ticket not yet Tech Complete,
/// Ready for Release or Done. The board has 170 roots but about 30 with open
/// work, and those are the ones a QA filters by. Busiest first, then
/// `No parent`, which is always offered.
pub fn root_filter_options(items: &[Item]) -> Vec<(String, String)> {
    let index = RootIndex::new(items);
    let mut open: HashMap<String, (String, u8, usize)> = HashMap::new();
    for item in items.iter().filter(|i| !i.status.is_completed()) {
        let root = index.resolve(item);
        if matches!(root, Root::None) {
            continue;
        }
        let entry = open
            .entry(root.filter_value())
            .or_insert_with(|| (root.filter_label(), root.rank(), 0));
        entry.2 += 1;
    }
    let mut options: Vec<(String, (String, u8, usize))> = open.into_iter().collect();
    // Busiest first; epics before other roots on a tie; then by label so the
    // order is stable between renders.
    options.sort_by(|a, b| {
        b.1 .2
            .cmp(&a.1 .2)
            .then(a.1 .1.cmp(&b.1 .1))
            .then(a.1 .0.cmp(&b.1 .0))
    });
    let mut out: Vec<(String, String)> = options
        .into_iter()
        .map(|(value, (label, _, _))| (value, label))
        .collect();
    out.push((NO_PARENT.to_string(), "No parent".to_string()));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::status::Status;

    fn issue(repo: &str, number: u32, title: &str, parent: Option<u32>) -> Item {
        Item {
            id: format!("{}-{}", repo, number),
            title: title.to_string(),
            repository: Some(repo.to_string()),
            number: Some(number),
            parent_number: parent,
            parent_title: parent.map(|p| format!("parent {}", p)),
            status: Status::InQA,
            ..Default::default()
        }
    }

    const REPO: &str = "acme/board";

    #[test]
    fn test_index_resolves_like_the_scan() {
        let items = vec![
            issue(REPO, 1, "[Epic] Dave project", None),
            issue(REPO, 2, "Story", Some(1)),
            issue(REPO, 3, "Task", Some(2)),
            issue(REPO, 4, "Bug", Some(99)),
            issue("acme/other", 1, "Same number elsewhere", None),
        ];
        let index = RootIndex::new(&items);
        for item in &items {
            assert_eq!(
                index.resolve(item).filter_value(),
                resolve_root(item, &items).filter_value(),
                "#{:?}",
                item.number
            );
        }
        assert_eq!(index.resolve(&items[2]).filter_value(), "acme/board#1");
        assert_eq!(index.resolve(&items[3]).filter_value(), "acme/board#99");
        assert_eq!(index.resolve(&items[0]).filter_value(), NO_PARENT);
    }

    #[test]
    fn test_filter_label_names_the_root() {
        let items = vec![
            issue(REPO, 3111, "[Epic] Dave project", None),
            issue(REPO, 2, "Task", Some(3111)),
            issue(REPO, 3, "Bug", Some(4101)),
        ];
        assert_eq!(
            resolve_root(&items[1], &items).filter_label(),
            "#3111 Dave project"
        );
        // Off the board: named from the synced parent title.
        assert_eq!(
            resolve_root(&items[2], &items).filter_label(),
            "#4101 parent 4101"
        );
        assert_eq!(resolve_root(&items[0], &items).filter_label(), "No parent");
    }

    #[test]
    fn test_root_options_only_offer_roots_with_open_work() {
        let mut done = issue(REPO, 20, "Shipped task", Some(10));
        done.status = Status::Done;
        let items = vec![
            issue(REPO, 1, "[Epic] Busy", None),
            issue(REPO, 2, "a", Some(1)),
            issue(REPO, 3, "b", Some(1)),
            issue(REPO, 10, "[Epic] Finished", None),
            done,
            issue(REPO, 30, "Issues sprint 31", None),
            issue(REPO, 31, "c", Some(30)),
        ];
        let options = root_filter_options(&items);
        let values: Vec<&str> = options.iter().map(|(v, _)| v.as_str()).collect();
        // Busiest first, the finished epic left out, No parent always last.
        assert_eq!(values, vec!["acme/board#1", "acme/board#30", NO_PARENT]);
        assert_eq!(options[0].1, "#1 Busy");
    }
}
