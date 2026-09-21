//! Print today's QA Daily Standup + Sprint Report from the live board,
//! without starting the TUI. Preview tool for checking report changes
//! against the current sprint before shipping them.
//!
//!   cargo run --example daily_report

use git_tui::app::App;
use git_tui::cache::Cache;
use git_tui::gh;
use git_tui::gh::timeline::{BatchTimelineEntry, TimelineData};
use git_tui::handlers::helpers::split_repo;
use git_tui::{export, loader};

fn apply(item: &mut git_tui::models::item::Item, td: TimelineData) {
    item.status_history = Some(td.status_changes);
    item.comment_history = Some(td.comments);
    item.label_history = Some(td.labels);
}

fn main() -> anyhow::Result<()> {
    let mut app = App::new();

    let auth = gh::auth::check();
    app.current_user = auth.username.clone().unwrap_or_default();
    app.config.current_user = app.current_user.clone();
    eprintln!("[i] user: {:?}", app.current_user);

    loader::load_data_step_items(&mut app)?;
    loader::load_data_step_fields(&mut app);
    loader::load_data_step_project(&mut app);
    app.search_results = (0..app.items.len()).collect();
    if let Some(s) = app.current_sprint_name() {
        app.board_sprint_filter = Some(s);
    }
    eprintln!(
        "[i] items: {}, fields: {}, sprint filter: {:?}",
        app.items.len(),
        app.fields.len(),
        app.board_sprint_filter
    );

    // Hydrate timelines for the sprint synchronously (the TUI does this on a
    // background thread; the report reads status/label/comment history).
    let timeline_cache = Cache::new(1800);
    let owner = app.config.owner.clone();
    let sprint = app.board_sprint_filter.clone();
    let targets: Vec<(usize, String, u32)> = app
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| {
            i.repository.is_some()
                && i.number.is_some()
                && (sprint.is_none() || i.sprint.as_deref() == sprint.as_deref())
        })
        .map(|(idx, i)| (idx, i.repository.clone().unwrap(), i.number.unwrap()))
        .collect();
    eprintln!("[i] sprint items needing timeline: {}", targets.len());

    let mut batch: Vec<BatchTimelineEntry> = Vec::new();
    let mut from_cache = 0usize;
    for (idx, repo, number) in &targets {
        let key = loader::timeline_cache_key(repo, *number);
        let (cached, fresh) = timeline_cache.get_with_freshness::<TimelineData>(&key);
        match (cached, fresh) {
            (Some(td), true) => {
                apply(&mut app.items[*idx], td);
                from_cache += 1;
            }
            _ => {
                let (o, n) = split_repo(repo, &owner);
                batch.push(BatchTimelineEntry {
                    item_idx: *idx,
                    owner: o,
                    repo_name: n,
                    issue_number: *number,
                });
            }
        }
    }
    eprintln!(
        "[i] timeline: {} fresh from cache, {} to fetch",
        from_cache,
        batch.len()
    );

    for (n, chunk) in batch.chunks(20).enumerate() {
        eprintln!(
            "[i] fetching timeline batch {} ({} issues)...",
            n + 1,
            chunk.len()
        );
        match gh::timeline::fetch_status_changes_batch(chunk) {
            Ok(results) => {
                for (item_idx, td) in results {
                    if let Some((_, repo, number)) = targets.iter().find(|(i, _, _)| *i == item_idx)
                    {
                        let _ = timeline_cache.set(&loader::timeline_cache_key(repo, *number), &td);
                    }
                    apply(&mut app.items[item_idx], td);
                }
            }
            Err(e) => eprintln!("[!] batch {} failed: {}", n + 1, e),
        }
    }

    println!("\n================ DAILY STANDUP (Slack) ================\n");
    println!("{}", export::daily_standup_slack(&app));
    println!("\n================ DAILY STANDUP (Markdown) ================\n");
    println!("{}", export::daily_standup_md(&app));
    println!("\n================ SPRINT REPORT (Slack) ================\n");
    println!("{}", export::sprint_report_slack(&app));
    Ok(())
}
