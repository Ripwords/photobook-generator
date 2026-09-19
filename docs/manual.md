# PhotobookGen manual

How to use every screen of the app. For installing, see the [README](../README.md#install).

## Updates

PhotobookGen keeps itself current. It asks GitHub for a newer release once at
launch, and again whenever you press **Check for updates** in **Settings → About** (see [The window](#the-window)), then
downloads the one it finds and restarts into it. An update whose signature does not match
this project's key is refused, so a tampered download cannot install itself. The launch
check is quiet: offline, or with nothing newer out, it says nothing at all.

## The window

The window is a sidebar and one screen beside it. The sidebar is always there (⌘B hides
and shows it): **New photobook** at the top, **Library** under it, then **Drafts** (books
still being analysed or chosen, shown only when there are any), then **Favourites** (books you
starred, shown only when there are any), then every other book you have made, each with its
cover, and **Settings** at the bottom. Click a book or a draft there to open it from wherever
you are. Right-click a book there for the same menu the library gives it (see below). Hover any icon button for its name and, where it has one, its shortcut.

| Shortcut | Does | Where |
|---|---|---|
| ⌘N | New photobook (name it, then choose photo folders) | Anywhere |
| ⌘B | Show or hide the sidebar | Anywhere |
| ⌘, | Settings | Anywhere |
| ⌘J | Show or hide the chat | Book editor |
| ⌘E | Export | Book editor |

**Settings** has three tabs. **General** sets the appearance (System, Light or Dark),
manages storage, and lists the shortcuts above. **API keys** is where the chat's keys are saved. **About**
shows the version and what leaves this Mac: no image ever does, and the chat sends only a
description of the book (its layouts, page numbers and photo tags). With **Split chapters
by place** on, each chapter's centre (a latitude and longitude, never a photo) goes to Apple
to name its town, and nothing goes with the switch off. **About** also holds
updates. **Check for updates** asks now and answers either way, and a waiting update shows
its version and release notes there with **Install and restart** beside **Not now**. When
the quiet launch check is the one that found it, a dot appears on **Settings** in the
sidebar and clicking through opens this tab.

**Storage** (in **General**) shows how much disk the analysis cache uses against its
limit. The cache is every analysed photo's results plus the small preview image the contact
sheet shows, about 34 KB per photo; your original photos are never in it and never touched.
The limit is 2 GB unless you pick another (500 MB, 1 GB, 2 GB, 5 GB or 10 GB). When the cache
is over it, the app removes the photos no book or draft uses, least recently analysed first,
at startup and after each analysis. It never removes a photo that a saved book, a book
deleted within the last 30 days, a draft, or the photos on screen use, because the book
could not open without it. If those alone are over the limit, the section says so and
nothing they use is removed. **Clear unused** removes every photo nothing uses, straight
away. A removed photo is analysed again the next time its folder is chosen. Results
from an older version of the analyser are removed regardless of the limit, since the app can
no longer use them.


## 1. The library

The screen the app opens on: every book you have made, as covers built from its own photos.
The two buttons at the top right switch between covers and a list, which also shows each
book's source folders and last export. Search matches a book's name or any of its folder
names; the sort menu orders by last edited, date created, or name.

Click a book to open it. Right-click it, or hover and click **…**, for **Open**, **Add to
favourites** (or **Remove from favourites**), **Rename…**, **Show photos in Finder** (a
submenu naming each folder when the book draws on several), **Show export in Finder** (only
once the book has been exported) and **Delete…**. A starred book shows a star beside its name
and sits in the sidebar's **Favourites** group; starring does not change its last-edited
date. In the library, Rename edits the name in place: Return saves, Escape cancels. From the
sidebar it opens a small dialog instead, and renaming the book you have open updates its
title in the editor too. Deleting the book you have open returns you to the library. Deleting asks
first, then removes the book, with a notice offering **Undo**. A deleted book is kept for 30
days before its layout, your include/exclude decisions and its export history are gone for
good, though only that notice can bring it back. Files you already exported to disk are not
touched. The photo analysis cache keeps the book's photos while it is in the trash; after
that they count as unused, so reopening those folders re-analyses only the photos that
**Storage** has since removed.

With no books yet, the library is a short explanation of the three steps and a **New
photobook** button, which does the same as the one in the sidebar.

**New photobook** asks for the book's name first, then its photo folders (**Choose
folders…**, and **Add folders…** for more; **×** removes one). **Start** is enabled once
there is a folder. A blank name becomes the first folder's name, which the field shows as its
placeholder.

## 2. Choosing the photos

Starting a book opens it as a **draft**. Every supported image in them and in their subfolders is analysed
on this Mac (nothing leaves the machine). Several folders are one set: every photo is ranked
against all the others, and photos shot the same afternoon fall into one event whichever
folder they came from.

When you shot the same picture several times within two minutes, the app keeps one: the
sharpest, unless it is almost entirely black or blown out and a properly exposed frame
exists.

The contact sheet shows every photo with the ones the book would leave out dimmed. Use **+**
and **−** on a photo to include or exclude it yourself. The bar above the sheet stays pinned
while you scroll it: **All photos** or **Keepers only**, the keeper count, and the tile
size. The panel on the
right is the book itself: name it, pick a length and, under **Print size**, **Change…** the
page size (the same panel as in the editor, without the dry run), and below that are the selection
counts and a key to the marks on the tiles. **Choose different folders** at the top starts
again from other folders. The app recommends the shortest Pixajoy length that fits the keepers and says
how many each length would leave out.

**Split chapters by place**, under **Print size**, is off for a new book. The book is laid
out in chapters, one per event, and with the switch off a chapter ends only at a break of more
than four hours. With it on, a chapter also ends when you move to another town, so a day in
Kyoto and an evening in Osaka are separate chapters on separate spreads. A chapter too small to
fill a spread of its own, such as one photo taken from a train, still joins its neighbour. Two places count
as different towns when they are more than 25 km apart, which keeps a whole city as one
chapter. One stray location does not start a chapter: the photo after it has to be far away
too. The contact sheet regroups as soon as you switch it on. It uses the location your camera
or phone recorded in each photo. Photos with no location stay in the chapter they were taken
in. When none of the photos has a location, the switch is disabled and says so.

With the switch on, the contact sheet titles each chapter with its town, such as **Kyoto
5 photos**. To find the town, the app sends the centre of each chapter, as a latitude and
longitude, to Apple's map service. It never sends a photo, and it sends nothing while the
switch is off. Names are kept on this Mac, so reopening a draft asks Apple nothing new. A
chapter Apple cannot name, or any chapter while this Mac is offline, keeps its number, such
as **Event 3**.

The book aims for variety over density. It places about four photos per spread, so a
40-page book places around 85 rather than the 130 its layouts could squeeze in, and it mixes
sparse spreads with the occasional six-up. Photos taken within two minutes of each other
count as one moment: the book takes the best photo of every moment before a second from any,
and never more than two from one moment unless you marked the others **+**. The photos this
leaves out are still one click away in the editor, under **Choose any photo…**. A length that cannot hold every photo you explicitly
marked **+** cannot be generated at all, and says so instead of quietly dropping one.

**Generate book** saves the book, opens it in the editor, and removes the draft.

You can leave a draft at any point, including while it is still being analysed. It stays in
the sidebar's **Drafts** group: a spinner and a photo count while it analyses, **Ready** when
it is done, **Failed** if it could not be analysed. Its name and your include and exclude
choices are kept, so you can start another book, or open one, while it works; several drafts
analyse at once, taking turns. When one finishes while you are elsewhere, a notice says it is
ready, with **Open** to go to it. The trash button at the top right of the draft,
**Discard draft**, throws it away after asking.

Drafts are saved as you go, with their names, folders, print size, **Split chapters by
place** and your choices, and come back when
the app is reopened. A draft that was still analysing when you quit starts again; the
analysis is cached, so photos it already reached are quick.

Arriving here from **Edit photos** on a saved book is the same screen, as a draft named after
that book, with its decisions, print size and **Split chapters by place** restored once the
analysis is done (**Edit photos** again
reopens the same draft), and the generate control becomes two: **Update "<name>"** replaces the
saved book (its export history goes with it), **Save as a new photobook** keeps both.
Choosing different folders makes it a new book again, since a book built from a different
source is not an update of the old one.

## 3. Editing the book

The book, spread by spread, as it will print. **Everything on this screen is written to disk
as you do it** (the check beside the title says when it last saved), so there is nothing to
save and you can leave whenever you like through the sidebar.

The toolbar at the top holds the title, with a pencil to rename the book; **Edit photos**,
which takes its photo selection back to the contact sheet and re-analyses the book's own
folders (every photo a cache hit, no new Vision work); **Chat** (⌘J), which shows or hides
the chat beside the book; and **Export** (⌘E). The bar along the bottom starts with the book's
print size (for example **11 x 8.5 in**), then counts its pages, the photos placed, any blank
pages and the photos left out, and says what dragging a photo does in the current mode.

Every opening (page 1, each pair of facing pages, the last page) has four controls on its
right. Page 1 is shown facing the inside front cover and the last page facing the inside
back cover, the way the printed book opens.

- **Regenerate** (dice) lays the same photos out on a layout this opening has not shown
  yet. Click again to see the next one; when every layout for that many photos has been
  shown, the button is disabled.
- **Reject** (thumbs down) does the same, and never offers the current layout here again.
- **Choose a layout** (grid) lists the other layouts that hold this many photos by name.
- **Lock** keeps the opening exactly as it is: **Shuffle** at the top of the book re-lays
  every unlocked opening at once and skips locked ones, and a locked opening refuses every
  other change until unlocked.

To **swap two photos**, click one, then click another anywhere in the book; click the
first again or press Escape to cancel. To **use a photo the book left out**, click the photo
to replace, then **Choose any photo…** in the bar at the bottom. The dialog opens on the
photos no page uses, each shown cropped the way that slot would print it; **In the book** and
**All** show the rest, sorted **Best first** or by **Time taken**. Pick one and press
**Replace**, or double-click it. Picking a photo that is already on another page reads **Swap
with page N** and exchanges the two. A photo that would cut a face, put one in the fold or
the trim margin, or print too small is dimmed with the reason and cannot be picked, and so
are photos on a locked spread. To **adjust a crop**, drag the photo inside its
slot to move the window, or hold ⌘ and scroll over it to zoom;
a plain scroll always scrolls the book, never a photo. The window keeps the slot's shape and
is saved when you let go. Regenerating that opening or swapping the photo away recomputes
its crop, because the slot it was chosen for is gone.

To **move or resize the boxes themselves**, switch the bar above the book from **Crop and
swap** to **Move and resize boxes**. Drag a box to move it, drag a corner to resize it; edges snap to the trim, safe and
gutter guides, the page edge (which prints as bleed) and the other boxes on the page. A box
cannot leave the page, shrink below 5% of it, or overlap another box, and the photo is
re-cropped for the new shape under the same rules as everything else. Switch back to **Crop
and swap** to go back to swapping and cropping. A change that would cut a face, put one in the
gutter or the safe margin, or print below the book's lowest print resolution is refused with
the reason, and the book stays as it was.

**The cover** sits above page 1, laid flat the way it prints: the back cover on the left, the
spine in the middle and the front cover on the right. Each side is drawn at the real shape
of one cover panel, which is the finished board plus the **wrap**, the band of photo that
runs past the trim on the top, bottom and outer edge and folds under the board (see **Cover
wrap** under **Print size** below). The wrap is shaded: it prints but never shows on the
finished book, so nothing that matters should sit in it. The red dashed line is where the
board ends and the blue one is the safe margin inside it, as on the pages. There is no wrap
at the spine edge, because that edge does not fold. The spine is drawn at a nominal width in
its colour, because Pixajoy publishes no formula for how wide it prints.

Both cover photos are optional and a new book has none. An empty side reads **Choose a front
cover photo** (or back); click it to open the same photo dialog as a page, titled **Choose
the front cover photo**, with every analysed photo shown cropped to the cover's shape.
Any photo can go on the cover, including one already on a page: the cover uses a copy and
the pages are left alone. Pick one and press **Use on the front cover**, or double-click it.
A photo that would put a face in the wrap or too near the board's edge, cut a face, or print
too small for the panel is dimmed with the reason and cannot be picked. Click a cover photo
to change it, or to take it off with **Remove** at the bottom of the dialog. Drag a cover
photo to move its crop and hold ⌘ and scroll over it to zoom, exactly as on a page; a crop
that would cut a face, push one into the wrap or too near the board's edge, or zoom in below
the lowest print resolution is refused with the reason, and the cover stays as it was.

**Spine colour**, above the cover on the right, opens the system colour picker; the chosen
colour is shown beside it as a hex value (for example `#8a5a2b`) and fills the spine in the
drawing. It is one plain colour. The app offers no suggested swatches.

**Print size.** Click the size at the left of the bottom bar to open the **Print size** panel
beside the book. It holds what a printer publishes: the **Book size** (width and height of
the finished page after trimming), the **Bleed** on the three outer edges, the **Safe margin**
inside the trim and the **Fold** strip measured in from the binding, and the **Lowest** and
**Target** print resolution in DPI. Below the lowest, export stops; below the target, it
warns. Under **Cover**, **Cover wrap** is how far the front and back cover photos run past
the trim to fold around the board, on the top, bottom and outer edge but not at the spine
(Pixajoy: 0.75 in, at most 3 in). It sets the shape the cover photos are cropped to, and
faces are kept out of the part that folds under. A book or draft saved before this field
existed opens with 0.75 in. **in** and **mm** at the top switch every length between inches and millimetres; that
is a display choice only and never changes the book. At the top of the panel, a drawing of
one spread labels the trimmed width and height and shades the bleed (red), safe margin (blue)
and fold (amber) in the same colours as the editor's guides, with each value in the legend.
The band for the field you are editing lights up. Thin margins are drawn wider than scale so
they stay visible, and the drawing says when it has done that; the numbers are exact. While
a field holds something the app cannot build with, the drawing dims and keeps the last valid
size. While you type, the book behind the
panel redraws its page shape and trim, safe and fold guides at the new numbers, and the panel
says what would fail at that size, for example "At this size, 3 problems would block the
export", with the list. Anything the app cannot build with (a blank field, a margin that
leaves no room for photos, a target resolution at or under the lowest) is refused with the
reason and **Change print size** stays disabled. When the dry run finds problems the button
reads **Change print size anyway**: the change is always allowed, and export will block
until they are fixed. Applying keeps every layout, lock, swap and moved box; only the crops
are recomputed when the page's shape changed, including crops you adjusted by hand. The
cover photos are kept too, and re-cropped the same way when the cover panel's shape changed
(a new book size or cover wrap); the panel says which crops a change will recompute before
you apply it.
**Shuffle** or **Regenerate** re-lay openings against the new size if you want that.
**Reset to Pixajoy 11 x 8.5** appears once the numbers differ from the default.

The built-in layouts were drawn for a landscape page. A portrait or square size is allowed,
and the panel says so: the layouts still fit but were not composed for that shape. A size is
set per book. A new book starts at Pixajoy's size, not at the last book's; set it on the
draft screen under **Print size** before generating, or here afterwards. Starting new books
from the last book's size is a possible later change, not an oversight.

**Export** opens a sheet from the right. Choose an output folder and export. Pre-flight
runs first, at the book's print size; anything that would print badly blocks the export and
is listed in the sheet. It checks each cover photo too, and its findings name the side
instead of a page: a cover photo whose source file has moved, or whose face would fall in
the wrap or be cut, blocks the export, and one that resolves below the lowest print
resolution across the whole panel blocks it, while one below the target only warns. The
output is one cropped file per photo placement plus a `manifest.json` saying what went where,
ready to upload to the printer. Each cover photo is written as its own file, cropped to the
full panel including the wrap, named `cover-front-` or `cover-back-` followed by the start of
the photo's hash (for example `cover-front-3fa9c21b.jpg`), so both sort ahead of the pages.
A JPEG or HEIC source is written as `.jpg` and any other source, a RAW file included, as
`.png`, the same as the page files.
The manifest's `cover` entry records the panel size in inches (`panel_w_in`, `panel_h_in`),
each side's file, source and crop (`null` for a side with no photo), and the spine colour as
`spine_hex`.

**Pixajoy.** Pixajoy's cover is set up in its own cover editor, not uploaded with the pages.
Upload the `cover-front-` and `cover-back-` files there and place each on its side, then set
the spine colour to the `spine_hex` value from the manifest. Pixajoy publishes no formula for
the spine's width, so the app does not produce a spine image or a single wrap-around file;
the spine is Pixajoy's to size.

