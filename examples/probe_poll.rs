//! Times each background poller used by the main poll thread.
//!
//! Run with: `cargo run --release --example probe_poll -- 10`
//!
//! Each poller is exercised the same way `main.rs` drives it, so the printed
//! per-call cost can be turned into "% of one core" by dividing by the cadence.

use std::time::Instant;

fn ms(f: impl FnOnce()) -> f64 {
    let t0 = Instant::now();
    f();
    t0.elapsed().as_secs_f64() * 1000.0
}

/// Process CPU time in ms (probe is single-threaded, so this is the call's CPU cost).
fn cpu_ms() -> f64 {
    use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    use windows::Win32::Foundation::FILETIME;
    unsafe {
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        let _ = GetProcessTimes(GetCurrentProcess(), &mut c, &mut e, &mut k, &mut u);
        let to_i64 = |f: FILETIME| ((f.dwHighDateTime as i64) << 32) | f.dwLowDateTime as i64;
        (to_i64(k) + to_i64(u)) as f64 / 10_000.0
    }
}

/// Subscribes to `UserNotificationListener::NotificationChanged` and prints every
/// event for `secs` seconds. Used to prove the event path fires before relying on it.
fn watch(secs: u64) {
    use std::sync::{Arc, Mutex};
    use windows::Foundation::TypedEventHandler;
    use windows::UI::Notifications::Management::UserNotificationListener;

    let Ok(listener) = UserNotificationListener::Current() else {
        println!("no listener");
        return;
    };
    if listener.RequestAccessAsync().ok().and_then(|o| o.get().ok())
        != Some(
            windows::UI::Notifications::Management::UserNotificationListenerAccessStatus::Allowed,
        )
    {
        println!("access not allowed");
        return;
    }
    let hits: Arc<Mutex<Vec<(u32, i32, f64)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = hits.clone();
    let handler = TypedEventHandler::<
        UserNotificationListener,
        windows::UI::Notifications::UserNotificationChangedEventArgs,
    >::new(move |_, args| {
        if let Some(a) = args {
            let id = a.UserNotificationId().unwrap_or(0);
            let kind = a.ChangeKind().map(|c| c.0).unwrap_or(-1);
            // Time the per-id fetch that replaces the full-list poll.
            let t0 = Instant::now();
            if let Ok(l) = UserNotificationListener::Current() {
                let _ = l.GetNotification(id);
            }
            let fetch_ms = t0.elapsed().as_secs_f64() * 1000.0;
            sink.lock().unwrap().push((id, kind, fetch_ms));
        }
        Ok(())
    });
    let token = listener.NotificationChanged(&handler).unwrap();
    println!("subscribed, waiting {secs}s for events (fire a test toast now)...");
    let deadline = Instant::now() + std::time::Duration::from_secs(secs);
    while Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let mut h = hits.lock().unwrap();
        for (id, kind, fetch_ms) in h.drain(..) {
            println!("EVENT id={id} change_type={kind} GetNotification={fetch_ms:.2}ms");
        }
    }
    let _ = listener.RemoveNotificationChanged(token);
    println!("done");
}

/// Removes only the toasts created by this probe ("CyNotch probe" in the title),
/// so benchmarking never counts the test load or pollutes the user's Action Center.
fn clean_probe_toasts() {
    use windows::UI::Notifications::NotificationKinds;
    use windows::UI::Notifications::Management::UserNotificationListener;
    let Ok(l) = UserNotificationListener::Current() else {
        return;
    };
    let Ok(op) = l.GetNotificationsAsync(NotificationKinds::Toast) else {
        return;
    };
    let Ok(list) = op.get() else { return };
    let mut removed = 0;
    for n in list {
        let Ok(id) = n.Id() else { continue };
        let mut text = String::new();
        if let Ok(nd) = n.Notification()
            && let Ok(vis) = nd.Visual()
            && let Ok(bindings) = vis.Bindings()
        {
            for b in bindings {
                if let Ok(elems) = b.GetTextElements() {
                    for el in elems {
                        if let Ok(t) = el.Text() {
                            text.push_str(&t.to_string_lossy());
                        }
                    }
                }
            }
        }
        if text.contains("CyNotch probe") {
            let _ = l.RemoveNotification(id);
            removed += 1;
        }
    }
    println!("probe toasts removed: {removed}");
}

/// Splits the notification poll cost into: fetch list, read ids, read visual XML.
fn split() {
    use windows::UI::Notifications::Management::UserNotificationListener;
    use windows::UI::Notifications::NotificationKinds;
    let Ok(l) = UserNotificationListener::Current() else {
        return;
    };
    let fetch = || -> Option<_> {
        l.GetNotificationsAsync(NotificationKinds::Toast).ok()?.get().ok()
    };
    struct Row {
        n: u32,
        fetch: f64,
        size: f64,
        ids: f64,
        visual: f64,
    }
    let mut rows = Vec::new();
    for _ in 0..7 {
        let t0 = Instant::now();
        let list = fetch();
        let t_fetch = t0.elapsed().as_secs_f64() * 1000.0;
        let Some(list) = list else { return };

        let t1 = Instant::now();
        let n = list.Size().unwrap_or(0);
        let t_size = t1.elapsed().as_secs_f64() * 1000.0;

        let t2 = Instant::now();
        let mut ids = Vec::new();
        for i in 0..n {
            if let Ok(item) = list.GetAt(i) {
                ids.push(item.Id().unwrap_or(0));
            }
        }
        let t_ids = t2.elapsed().as_secs_f64() * 1000.0;

        let t3 = Instant::now();
        for i in 0..n {
            if let Ok(item) = list.GetAt(i)
                && let Ok(nd) = item.Notification()
                && let Ok(vis) = nd.Visual()
            {
                let _ = vis.Bindings();
            }
        }
        let t_visual = t3.elapsed().as_secs_f64() * 1000.0;
        rows.push(Row {
            n,
            fetch: t_fetch,
            size: t_size,
            ids: t_ids,
            visual: t_visual,
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    let avg = |f: fn(&Row) -> f64| rows.iter().map(f).sum::<f64>() / rows.len() as f64;
    println!("toasts          : {}", avg(|r| r.n as f64));
    println!("GetNotificationsAsync().get() : {:>7.2} ms", avg(|r| r.fetch));
    println!("list.Size()                   : {:>7.2} ms", avg(|r| r.size));
    println!("GetAt(i) + Id() for all       : {:>7.2} ms", avg(|r| r.ids));
    println!(
        "GetAt(i) + Notification().Visual().Bindings() : {:>7.2} ms",
        avg(|r| r.visual)
    );
}

/// Prints every toast currently in Action Center (app, id, text) so benchmark
/// runs can be attributed to a real workload instead of test noise.
fn list_toasts() {
    use windows::UI::Notifications::Management::UserNotificationListener;
    use windows::UI::Notifications::NotificationKinds;
    let Ok(l) = UserNotificationListener::Current() else {
        return;
    };
    let Ok(op) = l.GetNotificationsAsync(NotificationKinds::Toast) else {
        return;
    };
    let Ok(list) = op.get() else { return };
    println!("action center toasts: {}", list.Size().unwrap_or(0));
    for n in list {
        let id = n.Id().unwrap_or(0);
        let app = n
            .AppInfo()
            .ok()
            .and_then(|a| a.DisplayInfo().ok())
            .and_then(|d| d.DisplayName().ok())
            .map(|s| s.to_string_lossy())
            .unwrap_or_default();
        let mut text = String::new();
        if let Ok(nd) = n.Notification()
            && let Ok(vis) = nd.Visual()
            && let Ok(bindings) = vis.Bindings()
        {
            for b in bindings {
                if let Ok(elems) = b.GetTextElements() {
                    for el in elems {
                        if let Ok(t) = el.Text() {
                            text.push_str(&t.to_string_lossy());
                            text.push_str(" | ");
                        }
                    }
                }
                if !text.is_empty() {
                    break;
                }
            }
        }
        println!("  id={id} app={app:?} text={text}");
    }
}

/// Mimics the main poll loop cadence and compares the old blind 1 Hz toast poll
/// with the DB-signature-gated one. Prints CPU time, which is what the change targets.
fn sim_loop(secs: u64) {
    use cynotch::notifications::{
        notifications_db_signature, should_poll_toasts, SystemNotificationBridge,
    };

    let mut bridge = SystemNotificationBridge::new();
    if !bridge.is_connected() {
        let _ = bridge.try_init();
    }
    let run = |bridge: &mut SystemNotificationBridge, secs: u64, gated: bool| -> f64 {
        let mut tick: u32 = 0;
        let mut sig = None;
        let mut last_poll = Instant::now();
        let t0 = cpu_ms();
        let deadline = Instant::now() + std::time::Duration::from_secs(secs);
        while Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(500));
            tick += 1;
            if !tick.is_multiple_of(2) {
                continue;
            }
            if gated {
                let s = notifications_db_signature();
                if should_poll_toasts(sig, &s, last_poll.elapsed().as_secs()) {
                    let batch = bridge.poll_new_toasts();
                    sig = Some(notifications_db_signature());
                    last_poll = Instant::now();
                    std::hint::black_box(batch);
                }
            } else {
                let batch = bridge.poll_new_toasts();
                std::hint::black_box(batch);
            }
        }
        cpu_ms() - t0
    };

    let old = run(&mut bridge, secs, false);
    let new = run(&mut bridge, secs, true);
    let pct = |ms: f64| ms / (secs as f64 * 1000.0) * 100.0;
    println!("old (blind 1 Hz poll) : {old:8.1} ms CPU in {secs}s = {:5.2}% of one core", pct(old));
    println!("new (signature gated) : {new:8.1} ms CPU in {secs}s = {:5.2}% of one core", pct(new));
    println!("(the remaining new cost is the forced 5 s safety poll)");
    if old > 0.0 {
        println!("saving: {:.0}%", (1.0 - new / old) * 100.0);
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let iters: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(5);
    unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    if std::env::args().any(|a| a == "--sim-loop") {
        sim_loop(iters.max(5) as u64);
        return;
    }
    if std::env::args().any(|a| a == "--list") {
        list_toasts();
        return;
    }
    if std::env::args().any(|a| a == "--split") {
        split();
        return;
    }
    if std::env::args().any(|a| a == "--clean") {
        clean_probe_toasts();
        return;
    }
    if std::env::args().any(|a| a == "--watch") {
        watch(iters.max(5) as u64 * 4);
        return;
    }

    let mut media_poller = cynotch::media::MediaPoller::new();
    let mut audio_mon = cynotch::audio::AudioMonitor::new();
    let mut telemetry = cynotch::telemetry::TelemetryCollector::new();
    let mut notifs = cynotch::notifications::SystemNotificationBridge::new();
    let notif_status = notifs.try_init();
    println!("notification bridge status: {notif_status}");

    // Warm-up pass: first call pays COM/WinRT activation.
    let _ = media_poller.poll();
    let _ = audio_mon.read_master_volume();
    let _ = telemetry.poll();
    if notifs.is_connected() {
        let _ = notifs.poll_new_toasts();
    }

    let mut media_ms = 0.0;
    for _ in 0..iters {
        media_ms += ms(|| {
            let _ = media_poller.poll();
        });
    }
    let mut audio_ms = 0.0;
    for _ in 0..iters {
        audio_ms += ms(|| {
            let _ = audio_mon.read_master_volume();
        });
    }
    let mut telem_ms = 0.0;
    for _ in 0..iters {
        telem_ms += ms(|| {
            let _ = telemetry.poll();
        });
    }
    let mut notif_ms = 0.0;
    let mut notif_cpu_ms = 0.0;
    if notifs.is_connected() {
        for _ in 0..iters {
            let c0 = cpu_ms();
            notif_ms += ms(|| {
                let _ = notifs.poll_new_toasts();
            });
            notif_cpu_ms += cpu_ms() - c0;
        }
    } else {
        notif_ms = f64::NAN;
    }

    let n = iters as f64;
    let per = |v: f64| v / n;
    println!(
        "media.poll()        {:>8.2} ms/call  -> {:>5.2}% of one core at 2 Hz",
        per(media_ms),
        per(media_ms) * 2.0 / 10.0
    );
    println!(
        "audio.read_volume() {:>8.2} ms/call  -> {:>5.2}% of one core at 2 Hz",
        per(audio_ms),
        per(audio_ms) * 2.0 / 10.0
    );
    println!(
        "telemetry.poll()    {:>8.2} ms/call  -> {:>5.2}% of one core at 1 Hz",
        per(telem_ms),
        per(telem_ms) / 10.0
    );
    println!(
        "notif.poll_toasts() {:>8.2} ms/call  -> {:>5.2}% of one core at 1 Hz",
        per(notif_ms),
        per(notif_ms) / 10.0
    );
    println!(
        "  of which CPU      {:>8.2} ms/call  -> {:>5.2}% of one core at 1 Hz (rest is RPC wait)",
        per(notif_cpu_ms),
        per(notif_cpu_ms) / 10.0
    );
    if notifs.is_connected() {
        use windows::UI::Notifications::Management::UserNotificationListener;
        use windows::UI::Notifications::NotificationKinds;
        if let Ok(l) = UserNotificationListener::Current()
            && let Ok(op) = l.GetNotificationsAsync(NotificationKinds::Toast)
            && let Ok(list) = op.get()
        {
            println!("  action center toasts: {}", list.Size().unwrap_or(0));
        }
    }
    println!(
        "TOTAL poll thread   {:>8.2}% of one core",
        per(media_ms) * 0.2
            + per(audio_ms) * 0.2
            + per(telem_ms) * 0.1
            + per(notif_ms) * 0.1
    );
}
