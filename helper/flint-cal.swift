import Foundation
import EventKit
import AppKit

let store = EKEventStore()
var lastError = ""
let args = CommandLine.arguments

func ask(_ kind: EKEntityType) -> Bool {
    let sem = DispatchSemaphore(value: 0)
    var ok = false
    let done: (Bool, Error?) -> Void = { g, e in ok = g; if let e = e { lastError = e.localizedDescription }; sem.signal() }
    if kind == .event { store.requestFullAccessToEvents(completion: done) } else { store.requestFullAccessToReminders(completion: done) }
    _ = sem.wait(timeout: .now() + (args.contains("--interactive") ? 180 : 8))
    return ok
}

func emit(_ obj: [String: Any]) {
    let data = try! JSONSerialization.data(withJSONObject: obj)
    if let i = args.firstIndex(of: "--out"), i + 1 < args.count {
        try? data.write(to: URL(fileURLWithPath: args[i + 1]))
    } else {
        FileHandle.standardOutput.write(data)
    }
}

func work() {
    if args.count >= 3 && args[1] == "complete" {
        guard ask(.reminder), let r = store.calendarItem(withIdentifier: args[2]) as? EKReminder else { exit(1) }
        r.isCompleted = true
        try? store.save(r, commit: true)
        exit(0)
    }

    if args.count >= 3 && args[1] == "add" {
        guard ask(.reminder), let list = store.defaultCalendarForNewReminders() else { exit(1) }
        let r = EKReminder(eventStore: store)
        r.title = args[2]
        r.calendar = list
        if args.count >= 4, let t = Double(args[3]) {
            r.dueDateComponents = Calendar.current.dateComponents([.year, .month, .day, .hour, .minute], from: Date(timeIntervalSince1970: t))
        }
        try? store.save(r, commit: true)
        exit(0)
    }

    if args.count >= 7 && args[1] == "addevent" {
        guard ask(.event) else { exit(1) }
        let e = EKEvent(eventStore: store)
        e.title = args[3]
        e.startDate = Date(timeIntervalSince1970: Double(args[4]) ?? 0)
        e.endDate = Date(timeIntervalSince1970: Double(args[5]) ?? 0)
        e.location = args[6]
        if args.count > 7, !args[7].isEmpty { e.url = URL(string: args[7]) }
        e.calendar = store.calendars(for: .event).first(where: { $0.title == args[2] && $0.allowsContentModifications }) ?? store.defaultCalendarForNewEvents
        try? store.save(e, span: .thisEvent, commit: true)
        exit(0)
    }

    if args.count >= 5 && args[1] == "deleteevent" {
        guard ask(.event), let t = Double(args[4]) else { exit(1) }
        let start = Date(timeIntervalSince1970: t)
        let pred = store.predicateForEvents(withStart: start.addingTimeInterval(-60), end: start.addingTimeInterval(60), calendars: nil)
        if let e = store.events(matching: pred).first(where: { $0.title == args[3] && $0.calendar.title == args[2] && $0.calendar.allowsContentModifications && abs($0.startDate.timeIntervalSince(start)) < 1 }) {
            try? store.remove(e, span: .thisEvent, commit: true)
        }
        exit(0)
    }

    if args.count >= 10 && args[1] == "updateevent" {
        guard ask(.event), let t = Double(args[4]), let ns = Double(args[6]), let ne = Double(args[7]) else { exit(1) }
        let start = Date(timeIntervalSince1970: t)
        let pred = store.predicateForEvents(withStart: start.addingTimeInterval(-60), end: start.addingTimeInterval(60), calendars: nil)
        if let e = store.events(matching: pred).first(where: { $0.title == args[3] && $0.calendar.title == args[2] && $0.calendar.allowsContentModifications && abs($0.startDate.timeIntervalSince(start)) < 1 }) {
            e.title = args[5]
            e.startDate = Date(timeIntervalSince1970: ns)
            e.endDate = Date(timeIntervalSince1970: ne)
            e.location = args[8]
            e.url = args[9].isEmpty ? nil : URL(string: args[9])
            try? store.save(e, span: .thisEvent, commit: true)
        }
        exit(0)
    }

    var out: [String: Any] = ["names": [String](), "events": [Any](), "tasks": [Any](), "errors": [String]()]
    var errors: [String] = []

    if ask(.event) {
        let now = Date()
        let start = Calendar.current.date(byAdding: .day, value: -7, to: Calendar.current.startOfDay(for: now))!
        let end = Calendar.current.date(byAdding: .day, value: 45, to: start)!
        let cals = store.calendars(for: .event)
        out["names"] = Array(Set(cals.map { $0.title })).sorted()
        let pred = store.predicateForEvents(withStart: start, end: end, calendars: cals)
        out["events"] = store.events(matching: pred).map { e -> [String: Any] in
            ["calendar": e.calendar.title, "title": e.title ?? "", "start": e.startDate.timeIntervalSince1970, "end": e.endDate.timeIntervalSince1970, "location": e.location ?? "", "all_day": e.isAllDay]
        }
    } else { errors.append("Calendar access was not granted" + (lastError.isEmpty ? "" : " (" + lastError + ")")) }

    if ask(.reminder) {
        let sem = DispatchSemaphore(value: 0)
        let pred = store.predicateForIncompleteReminders(withDueDateStarting: nil, ending: nil, calendars: nil)
        var items: [Any] = []
        store.fetchReminders(matching: pred) { rs in
            items = (rs ?? []).map { r -> [String: Any] in
                var d: [String: Any] = ["id": r.calendarItemIdentifier, "list": r.calendar.title, "title": r.title ?? "", "flagged": r.priority == 1]
                if let c = r.dueDateComponents, let date = Calendar.current.date(from: c) { d["due"] = date.timeIntervalSince1970 }
                return d
            }
            sem.signal()
        }
        _ = sem.wait(timeout: .now() + 60)
        out["tasks"] = items
    } else { errors.append("Reminders access was not granted" + (lastError.isEmpty ? "" : " (" + lastError + ")")) }

    out["errors"] = errors
    out["status"] = ["calendar": EKEventStore.authorizationStatus(for: .event).rawValue, "reminders": EKEventStore.authorizationStatus(for: .reminder).rawValue]
    emit(out)

    exit(0)
}

let nsapp = NSApplication.shared
nsapp.setActivationPolicy(.accessory)
nsapp.activate(ignoringOtherApps: true)
DispatchQueue.global().async { work() }
nsapp.run()
