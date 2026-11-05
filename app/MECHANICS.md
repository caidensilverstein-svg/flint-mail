# Mechanics sheet

The original is an Electron app (version 3.32.2) with a native mail core. Its rules
live in the JavaScript bundles inside `Contents/Resources/app.asar` and in the
core's generated type list (`node_modules/sparkcore/dist/sparkcore-auto-generated-types.d.ts`).
Line numbers below refer to the pretty-printed files Craft Anything restored
(evidence ev_6631f1ca..., ev_14283414..., ev_63862d48...). "Estimated" means I looked
and did not find the value.

## Snooze and send later presets
Source: `dist/renderer/chunk__63094_51504b1f51b15764.js`, line 477, three preset tables (`s`, `a`, `o`).

| Preset | Value | Source |
|---|---|---|
| Later Today (snooze) | now + 3 h | table `s`, `LaterToday: RelativeHours 3` |
| Later Today (send later / reminder) | now + 2 h | tables `a`, `o`, `RelativeHours 2` |
| This Evening | 20:00 | `ThisEvening: Time 20:00` (all tables) |
| Tomorrow | 09:00 next day | `Tomorrow: Time 9:00` |
| Tomorrow Evening | 20:00, off by default | `TomorrowEvening ... isEnabled:false` |
| This Weekend | Saturday 09:00 | table `s` (tables `a`/`o` say Sunday) |
| Next Week | Monday 09:00 | tables `a`/`o` (table `s` says Sunday) |
| Someday | no date | `MessagesDataSourceItemSnoozeDateType.Someday`, types d.ts line 29921 |

Which table is snooze and which is send later is inferred from the 3 h / 2 h split
and the `Alert` entry (present only in `a`/`o`). This is still an open unknown.

## Menu bar (status item)
Source: `dist/renderer/chunk__81946_51504b1f51b15764.js` (TrayMenu, lines 181-262) and `dist/main/main.js` (TrayService, pretty lines 46420-46525).

- Menu items in order: Open app, Open Agenda (when agenda is on), New Email, Open Calendar (when a calendar account exists), Settings, Quit. (lines 184-219)
- Agenda refresh: every 60 000 ms (`F(6e4)`, line 183).
- Icon states: Default, MeetingInProgress, MeetingTranscribing (main.js 46313, 46472).
- In progress when `startDate - 60 000 ms <= now` (line 226).
- Left click opens an agenda popover 200 px left of the icon (line 256). The rebuild puts the agenda at the top of the menu instead.
- Title text uses monospaced digits (main.js 46508). What it shows is unknown; the rebuild shows the unread count and a countdown to a meeting within the hour (estimated).
- Dock menu: New Email, Open Calendar, Close All Secondary Windows, Settings (chunk__81946 lines 136-160).

## Smart Inbox
- Sections: Priority, People, Notifications, Newsletters, Pinned, Seen, New Senders, Shared Drafts, Shared Inbox, Assigned To Me (types d.ts line 14744).
- Card actions: none, mark as read, done, delete (d.ts line 19006).
- Card order on screen: Priority, People, Notifications, Newsletters, Pins, Seen (estimated).
- Items shown per card before "Show more": 3 (estimated; the setting exists as `SettingsSmartInboxCardItemsLimit`, d.ts line 19079).

## AI assistant
- Actions: generate message, generate reply, quick reply, rephrase, expand, shorten, spell check, break into paragraphs, adjust, edit, translate, custom (d.ts line 326).
- Tones: friendly, neutral, formal (d.ts line 320). Lengths: short, medium, long (d.ts line 889).
- Quick reply intents: interested, not interested, gratitude (d.ts line 287).
- Paid limits: free users hit a paywall, paid users get a refilling quota (d.ts lines 844, 895). The rebuild has no limit.

## Paid features (all unlocked in the rebuild)
Paywall reasons (d.ts line 25242): home screen, removing the signature footer, templates, group and thread actions, composer AI, integrations, my writing style, scheduling links ("meet with"), read receipts, translate, auto labels, auto drafts, shared inbox sending, AI persona. Plans: Plus, Pro, Frontier (d.ts line 8612).

## Integrations
Agenda, Asana, Bear, Evernote, MeisterTask, Notion, OmniFocus, Reminders, Things, TickTick, Microsoft To Do, Todoist, Trello, Zoom, Microsoft Teams, OneNote, Slack, Google Drive, Dropbox, HubSpot (d.ts line 2233).

## Calendar
- Views: Day, 3 Days, Week, Month (d.ts line 11901).
- Default view: Week (estimated).

## Colours
Source: `dist/renderer/app-colors.css`.
sidebar #282a2e, sidebar text #aeb8c5, sidebar secondary #66707e, sidebar selection #0070e6,
accent #157efb, deep accent #0053cf, text #141414, secondary text #66707e, selection #e5f2ff;
dark mode text #dee3eb, secondary #868d97, selection #193350.

## Settings defaults
Source: `dist/renderer/chunk__47621_51504b1f51b15764.js`, settings defaults table.
- Undo send delay: 5 s by default (`UndoSendDelay: Delay5`); choices 5, 10, 15, 30, 60 s (d.ts line 41421).
- Calendar shows 10 hours on screen (`CalendarNumberOfVisibleHours: 10`).
- Default calendar view: searched the same table and the types, not found (estimated Week).

## Values chosen for the rebuild (not in the original)
- Follow-up reminder choices: 1, 2, 3 or 7 days (estimated).
- Meeting notification: 60 s before start (same lead as the in-progress icon).
- IMAP fetch: newest 40 messages per account.
- AI: up to 1024 tokens per answer, default model claude-sonnet-5-5.
