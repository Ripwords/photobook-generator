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
| ⌘Z | Undo the last change | Book editor |
| ⇧⌘Z | Redo the last undone change | Book editor |
| ⌘J | Show or hide the chat | Book editor |
| ⌘E | Export | Book editor |

**Settings** has three tabs. **General** sets the appearance (System, Light or Dark) and
the graphics level, manages storage, and lists the shortcuts above. **API keys** is where the chat's keys are saved. **About**
shows the version and what leaves this Mac: no image ever does, and the chat sends only a
description of the book (its layouts, page numbers and photo tags). With **Split chapters
by place** on, each chapter's centre (a latitude and longitude, never a photo) goes to Apple
to name its town, and nothing goes with the switch off. **About** also holds
updates. **Check for updates** asks now and answers either way, and a waiting update shows
its version and release notes there with **Install and restart** beside **Not now**. When
the quiet launch check is the one that found it, a dot appears on **Settings** in the
sidebar and clicking through opens this tab.

**Graphics** (in **General**) sets how sharp photos look in the book editor: the pages, the
cover and the crop dialog. It never changes the exported book, which always uses your
original photos.

| Level | The editor shows | Cost for each photo the first time |
|---|---|---|
| **Low** | The small preview from analysis, which can look soft on a large screen | None |
| **Optimised** | A 1600 px image, read from the preview your camera saved inside the file | A fraction of a second, a few hundred KB of disk |
| **Maximum** | A 2400 px image, read from the full photo | About half a second for a RAW photo, more disk than Optimised |
| **Auto** | Maximum on a Mac with 16 GB of memory or more, otherwise Optimised | As the level it picks |

Auto is the default, and its card says which level it picked on this Mac. If the app cannot
read how much memory the Mac has, Auto is not offered and the default is Optimised. When a
book opens, every photo shows its small preview at once and turns sharp as its larger image
is made, the cover first and then the pages in order, without anything moving. A photo
already made at that level is shown straight away. The contact sheet always uses the small
previews. The larger images count toward the storage limit below and are removed with their
photo; switching to Low does not delete the ones already made.

**Storage** (in **General**) shows how much disk the analysis cache uses against its
limit. The cache is every analysed photo's results plus the small preview image the contact
sheet shows, about 34 KB per photo, and the larger images **Graphics** makes for the editor; your original photos are never in it and never touched.
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

Click a book to open it. Right-click it, or hover (or Tab to it) and click **…**, for **Open**, **Add to
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

The contact sheet dims every photo the book would leave out **at the length you have chosen**. A
longer book places more, so switching from 20 to 40 pages lights more of them up. Use **+**
and **−** on a photo to include or exclude it yourself. The bar above the sheet stays pinned
while you scroll it: **All photos** or **In the book**, the placed count, and the tile
size. Once a length is chosen, both the count and **In the book** switch to that length too,
so "N of M in the book" always matches what is undimmed and **In the book** hides exactly what
the book would leave out at the length you picked, not just what culling excluded. The panel on the
right is the book itself: name it, pick a length and, under **Print size**, **Change…** the
page size (the same panel as in the editor, without the dry run). At the bottom of the panel,
two closed sections open on a click: **How to read the sheet**, a key to the marks on the
tiles, and **Technical details**, the selection counts (keepers, analysed and scanned, plus
any that failed or came from the cache). Hover a tile and click the info button in its
bottom corner for that photo's aesthetic and sharpness percentiles, ranked against every
photo in the set, and the faces it found, if any; a photo shows "Not ranked yet" until every
photo in its folders has been analysed. **Choose different folders** at the top starts
again from other folders. The app recommends the shortest Pixajoy length that fits the keepers; the
**Length** menu names each option by its page count and, under it, how many events get their own
page and how many photos that length places. Every length it offers is one Pixajoy sells. Under
the panel's settings, one sentence sums up the length you picked: how many of the keepers fit
(and how many of those you marked **+**), how many would be left out, and whether this is the
recommended length or which one is. **Generate book** (or, when you came from a book's **Edit
photos**, **Update "<name>"** and **Save as a new photobook**) stays pinned to the bottom of
the panel however far it scrolls.

**Event tiers.** Every event lands in one of four tiers, which decides how much of it the
book places: **Featured** gets at least your floor (2 to 12 photos, 6 by default), **Normal**
gets at least 2, **Brief** gets at most your cap (1 to 3, 2 by default), and **Skip** gets
none. The app suggests a tier for each event and gives a reason, such as "Stands out from the
rest of the trip" or "Ranked 4 of 9". Each event's header in the contact sheet has one
tier menu, labelled with the tier and who chose it, such as **Featured (auto)**; hover it to
read the reason, which also heads the menu when you open it. A Skipped event also shows its
reason right in the header, since its tiles are dimmed and easy to pass over. Pick
**Featured**, **Brief** or **Skip** in the menu to set the tier yourself, or **Auto** to hand
it back to the app (while the app is choosing, that item names its pick, such as **Auto:
Normal**); the
menu and the events panel then show your choice as **(you)**. It is saved with the draft and, once you generate, with the book. Choosing a tier
yourself always shows what you chose, even where the control would otherwise have shown why
the app suggested something else. Marking a photo **+** always places it, even in a Skipped
event.

**The events panel**, above **Length**, lists every event at the chosen length: its tier and
who chose it, and either why (a Skipped or empty event) or how many of its photos made it in,
such as "6 kept → 4 placed". Click a row to scroll the contact sheet to that event. The
**Length** menu counts the same thing for each length it offers: its page count, such as "40
pages", and under that, how many events get a page to themselves (Featured and Normal --
Brief and Skipped events share space with others rather than getting one each) out of every
event the book has, and how many photos that length places, such as "12 of 19 events · 85
photos". If your tiers and **+** photos ask for more photos than a length can hold, the panel says so
under the list; the book still generates, giving every event you haven't skipped at least one
photo. Choosing a longer length that earns
another event a page of its own says so too, for example "At 60 pages, 2 more events get their
own pages."

**Split chapters by place**, under **Print size**, is off for a new book. The book is laid
out in chapters that follow your events, with two exceptions: a Brief event's photos fold
into a neighbouring chapter rather than getting one of their own, and a Skipped event places
nothing, so it has no chapter at all. With the switch off, a chapter otherwise ends only at a
break of more than four hours. With it on, a chapter also ends when you move to another town, so a day in
Kyoto and an evening in Osaka are separate chapters on separate spreads. A chapter too small to
fill a spread of its own, such as one photo taken from a train, still joins its neighbour. Two places count
as different towns when they are more than 25 km apart, which keeps a whole city as one
chapter. One stray location does not start a chapter: the photo after it has to be far away
too. The contact sheet regroups as soon as you switch it on. It uses the location your camera
or phone recorded in each photo. Photos with no location stay in the chapter they were taken
in. When none of the photos has a location, the switch is disabled and says so.

Photos are put in the order they were really taken, even across time zones. A phone records
the time zone with each photo, so an evening in Tokyo comes before the next afternoon in
London even when the clock times say otherwise. A camera that records no time zone takes the
zone of the nearest phone photo from the same day. When nothing on that day records a time
zone, the camera's own clock decides the order, so set the camera to local time when you
travel.

Each event that prints four or more photos is told with a beginning and an end. It opens
on its best wide shot (a landscape, a street, a skyline) from the first half of the event,
and closes on its calmest photo from the last part: a wide, warm, evenly exposed frame. At
most those two photos move; the rest stay in the order they were taken. The opening photo
takes the first or the largest box on its pages, and the closing photo the last. When two
boxes are the same size, the opening photo takes the one you read first. A spread
that holds two events reads left to right in the order they happened. **Regenerate**,
**Shuffle** and the other layout changes keep all of this, so trying another layout never
buries an event's opening photo in a small box.

An event told over several spreads also has a rhythm. It starts on a spread with fewer
photos, fills up through the middle, and ends on a quieter spread again. A Featured event
starts sparser still, so its opening photo has room. This only steers how photos are shared
between spreads: it never leaves a page blank or a photo out to keep the rhythm.

With the switch on, the contact sheet titles each chapter with its town, such as **Kyoto
5 photos**. To find the town, the app sends the centre of each chapter, as a latitude and
longitude, to Apple's map service. It never sends a photo, and it sends nothing while the
switch is off. Names are kept on this Mac, so reopening a draft asks Apple nothing new. A
chapter Apple cannot name, or any chapter while this Mac is offline, is titled by the dates
its photos were taken, such as **12–14 Mar 2026**, the same title the book prints for it. A
chapter whose photos carry no date keeps its number, such as **Event 3**. With the switch
off, every chapter is titled by its dates.

**Featured events get at least** and **Brief events get at most**, the two steppers in the
closed **Advanced** section under **Split chapters by place**, set the floor and cap the tiers above use: Featured from 2 to 12
photos (6 by default), Brief from 1 to 3 (2 by default). Changing either re-scores every
event at every length.

The book aims for variety over density. It places about four photos per spread, so a
40-page book places around 85 rather than the 130 its layouts could squeeze in, and it mixes
sparse spreads with the occasional six-up. Photos on a page never touch: even a layout that
bleeds off the page edge keeps a thin white gutter (0.2") between its photos. Photos taken
within two minutes of each other count as one moment: for an event that places anything,
the book takes the best photo of every moment before a second from any, and never more than
two from one moment unless you marked the others **+**.
A photo that looks like one the book already takes -- the same shot
retaken minutes later, or a time-lapse of one sky -- waits until every event has run out of
different pictures, and a tier's floor counts different pictures only: an event of twenty
frames of one star field is owed that picture once, not six times. Within an event, the
book also looks for different kinds of shot: a wide view of the place, people you can
recognise, and a close look at something (food, a sign, a flower). When an event places
three or more photos and its best few are all one kind, the best decent photo of a missing
kind takes the third place (and the second, if two kinds are missing). The event's best
photo always keeps first place. A Skipped event places
nothing regardless, moments included. The
photos this leaves out are still one click away in the editor, under **Choose any photo…**. A length that cannot hold every photo you explicitly
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
that book, with its decisions, print size, event tiers and **Split chapters by place**
restored once the analysis is done (**Edit photos** again reopens the same draft), and the
generate control becomes two: **Update "<name>"** asks you to confirm, then builds the book
again from these photos and settings. Changes you made in the editor -- swapped or replaced
photos, crops, layouts, moved boxes, the cover, chapter titles you typed, and photos added
from disk -- are replaced;
its undo history, export history and favourite star don't carry over, though files you
already exported are not touched. **Save as a new photobook** keeps both. Choosing different
folders makes it a new book again, since a book built from a different source is not an
update of the old one.

## 3. Editing the book

The book, spread by spread, as it will print. **Everything on this screen is written to disk
as you do it** (the cloud with a check, beside Undo and Redo, says it has saved; it reads **Saving…** while a change is being written), so there is nothing to
save and you can leave whenever you like through the sidebar.

Because it saves as you go, there is no closing without saving to fall back on, so **Undo**
and **Redo** (the two arrows beside that check, ⌘Z and ⇧⌘Z) are the way back. Each names the
change it would move over -- the button reads "Undo move" or "Undo photo replacement" --
and is greyed out when there is nothing that way. Moving a box and resizing one are named
apart, even though both are the same kind of change underneath. Changes the chat made are on the same
timeline and undo exactly like your own. The last fifty states of each book are kept, they
survive quitting the app, and making a fresh change after undoing discards whatever Redo
would have put back.

Two things sit outside that timeline. Renaming the book is not one of its states, so Undo
will not take a name back -- rename it again. And while you are typing, in the chat box or
in the name field, ⌘Z belongs to the text you are typing and does nothing to the book;
click the book first.

Opening a saved book does not re-read its folders -- it resolves the photos it holds from
what was analysed, so it opens instantly and works with the source drive unplugged. It does
count what the folders have gained, though, and if photos have been added since the book was
made a notice above the first spread says how many and offers **Edit photos**, which is the
way to bring them in. Dismissing the notice hides it until the next time the book is opened.
A photo you only renamed or moved within the folders is not counted: the book already has
it, so re-analysing could never clear it. Nor is one analysis has already given up on --
a file that will not open, or that Vision could not read. Those are counted separately and
the notice names them, because **Edit photos** leaves them out: it would reach the same
verdict and the same count would come straight back. Editing or replacing the file clears
the mark, and the photo counts as new again on the next open. If a folder cannot be read --
one you chose, or one inside it -- the count says "at least this many".

The toolbar at the top holds the title, with a pencil to rename the book; **Edit photos**,
which takes its photo selection back to the contact sheet and re-analyses the book's own
folders (every photo the book already holds is a cache hit; a photo added to the folder
since is analysed for the first time, which does take a moment); **Export** (⌘E); and, at the
far right, the chat button (a panel icon, ⌘J), which shows or hides the chat beside the book
(while a reply is coming in, its send button stops it, and a message you type waits in the
box until the reply has finished). The bar along the bottom starts with the book's
print size (for example **11 x 8.5 in**), then counts its pages and the photos placed. Blank
pages and photos left out are added as warnings, with a warning sign, only when there are
any. What dragging a photo does in each mode is in the tooltip of that mode's button, in the
bar above the book. When the editor is narrow (the chat open in a small window), that bar
shows its buttons as icons; hover one for its name or, for the two modes, what a drag does.

Each chapter starts with a heading above its first opening. The heading gives the chapter
number, its pages, its dates (when the title is not already the dates) and how many events
it merges. Below that are the chapter's title and, under it, the page its title spot is on.
A book with two or more chapters also has a **Chapters** menu (the list icon) in the bar
above the book. It lists every chapter's title and pages; pick one to scroll to it.

Titles are a guide, not part of the printed book: export never draws them. On the page, a
violet dashed box marks the **title spot**, with a tag naming the title, so you can see where
it would go if you add it in Pixajoy. The **Guides** legend lists it with the trim and safe
lines. When the opening has no empty space for it, the spot sits over a photo and the
heading says so.

To **change a chapter's title**, press the pencil beside it, type, and press Enter. Clicking
away also saves; Esc leaves the title as it was. Clear the box and save for no title: the
heading then reads **No title** and the page shows no title spot. A title you have typed
stays through **Regenerate** and **Shuffle**, but not through **Update** after **Edit
photos**, which rebuilds the book. Undo takes it back like any other change. **Use automatic
title** goes back to the place name or dates, and typing the automatic title back does the
same. How to add a title in Pixajoy is under **Chapter titles** further down.

Every opening (page 1, each pair of facing pages, the last page) has four controls on its
right. Page 1 is shown facing the inside front cover and the last page facing the inside
back cover, the way the printed book opens.

- **Regenerate** (dice) lays the same photos out on a layout this opening has not shown
  yet. Click again to see the next one; when every layout for that many photos has been
  shown, the button is disabled.
- **Reject** (thumbs down) does the same, and never offers the current layout here again.
- **Choose a layout** (grid) lists the other layouts that hold this many photos. Each
  one has a small drawing of the page or spread at the book's page shape, with a grey
  block for each photo, so you can see the layout before picking it. The menu's first
  line, **Now**, draws and names the layout the opening uses now. Its template id is
  printed under it, one per page where the two pages are laid out separately.
- **Lock** keeps the opening exactly as it is: **Shuffle** at the top of the book re-lays
  every unlocked opening at once and skips locked ones, and a locked opening refuses every
  other change until unlocked. Pressing or dragging a photo on a locked opening says so in
  a notice at the bottom right, with an **Unlock** button.

To **swap two photos**, drag one onto the other, anywhere in the book. While you drag, the
photo travels with the pointer, the photo under it gets a ring, and the slot you started
from shows the photo it would get, so you can see the swap before you let go. The photo
under the pointer reads **Swap**, or **Locked** if it is on a locked spread, where a drop
does nothing. Hold the photo near the top or bottom edge of the book to scroll towards a
spread that is out of view. Let go anywhere that is not a photo, or press Escape, and the
photo slides back with nothing changed. A press only counts as a drag once it has travelled
about 10px, so a click still just selects, however much the trackpad wobbles.

Clicking a photo selects it, with a ring, and the bar at the bottom offers **Swap with…**,
**Crop…** and **Choose any photo…**. Right-clicking a photo opens the same three without
selecting it. **Swap with…** is the way to swap without dragging: choose it, then click the
other photo. Escape backs out one step at a time, first the swap, then the selection.

After a swap or a replaced photo, the slots that changed flash, and a notice at the bottom
right names the pages, for example "Swapped photos on pages 3 and 12", with an **Undo**
button. Adding a photo to a page and putting one on the cover get the same notice. That matters most for a swap between spreads that are never on screen together. A
change the book refuses is reported the same way, with the reason. Each photo is re-cropped
for its new slot, so a crop you set by hand does not travel with the photo.

To **use a photo the book left out**, drag it from **Left out**, below the last spread. The
desk scrolls when you hold it near the top edge. Drop it on a photo to replace that photo:
the slot shows the incoming photo while you hover, the label under the dragged photo reads
**Replace**, and the replaced photo goes back to **Left out**. Drop it on a page's empty
space, where the whole page is outlined and the label reads **Add to page N**, to add it
there: the spread is laid out again with one more photo, or the change is refused with the
reason when no layout holds that many. Drop it on the cover (**Use on front cover**) to put
it there. A left-out photo shows in grey until you point at it. Locked spreads read
**Locked**, as for a swap.

You can also click the photo to replace, then **Choose any photo…**
in the bar at the bottom. The dialog opens on the
photos no page uses, each shown cropped the way that slot would print it; **In the book** and
**All** show the rest, sorted **Best first** or by **Time taken**. Pick one and press
**Replace**, or double-click it. Picking a photo that is already on another page reads **Swap
with page N** and exchanges the two. A photo that would cut a face, put one in the fold or
the trim margin, or print too small is dimmed with the reason and cannot be picked, and so
are photos on a locked spread.

To **fill a page that prints blank**, press **Add a photo…** in the middle of that page.
The same dialog opens, titled **Add a photo to page N**, on the photos no page uses, each
shown at the page's shape. Pick one and press **Add to page N**. The page is then laid out
again on the best layout for one more photo; on a spread, the photo on the facing page can
move or change size too. Photos already in the book are dimmed with **Already used**: adding one
would print it twice, so swap it in instead. The button is not shown on a locked spread,
and when no layout holds one more photo on that spread the editor says so and nothing
changes. **Add from disk…** works here as well. Undo takes the photo back off.

To **use a photo from anywhere on the machine**, press **Add from disk…** at the top of
that same dialog. The photo does not have to be in the folders this book was made from, or
in any folder the app has analysed. It is analysed on the spot, which takes a moment the
first time, and then appears as one more photo in the dialog, already picked, so you can see
how it would be cropped and whether anything refuses it before pressing **Replace** or
**Use on the front cover**. If the photo turns out to be one this slot refuses -- or one the
book already held and already has here -- it is shown with the reason rather than picked.
The same button is on the cover picker. One caveat: a photo added this way belongs to the
book, not to its folders, so if you later run **Edit photos** and **Update "<name>"**, the
book is rebuilt from the folders and hand-added photos are not carried across.

To **crop a photo**, double-click it, or click it and then **Crop…** in the bar at the
bottom. The crop dialog
shows the whole photo, with the part that prints lit and the rest dimmed. Drag the frame to
choose what prints, and zoom with the slider, the **−** and **+** buttons, or by scrolling or
pinching over the photo; the arrow keys nudge the frame. The frame keeps the slot's shape.
Nothing changes until **Save crop** (or Enter), and then as one change you can undo;
**Reset** goes back to the crop the dialog opened on, and **Cancel** or Escape closes it
without saving. Scrolling over the book itself always scrolls the book; it never zooms a
photo.

Regenerating that opening or swapping the photo away recomputes its crop, because the slot
it was chosen for is gone.

To **move or resize the boxes themselves**, switch the bar above the book from **Arrange
photos** to **Move and resize boxes**. Drag a box to move it, drag a corner to resize it;
edges and centres snap to the guides. An edge catches on the trim, safe and gutter guides,
on the page edge (which prints as bleed), and on the edges of the other boxes on the page.
Moving a box also catches its centre on the middle of the printed page and on the centre of
another box, so two boxes can be lined up on their middles rather than their sides. Resizing
catches only the edge you are dragging, because pulling the box's centre onto a line would
move that edge twice as far as the pointer went. Whichever line is holding the box is drawn
across the page while you drag and goes as soon as you let go, so a centre snap is visible
even though no centre line is drawn at rest. Hold
**shift** while dragging a corner to resize the box without changing its shape: it scales
about the opposite corner and keeps the proportions it started with. Snapping is off while
shift is held, because pulling one edge onto a guide is exactly what would change the shape.
Shift is read as you drag, so you can take or drop the lock part-way through. Nudges of a
few pixels work here: a drag counts from about three pixels, rather than the ten a click on
a photo has to clear in **Arrange photos** to count as a drag. Below that the box does not move
at all, so resting a hand on it does not quietly shift the layout. A box
cannot leave the page, shrink below 5% of it, or overlap another box, and the photo is
re-cropped for the new shape under the same rules as everything else. Switch back to **Arrange
photos** to go back to swapping and cropping. A change that would cut a face, put one in the
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

Generating a book fills the cover for you: the front gets the best-looking photo the book
keeps that fits the panel, the back gets the next best, from a different part of the trip
when there is one, and the spine takes the front photo's main colour. Both cover photos are
optional. A book saved before the cover existed opens with none and a white spine, and either side can be
emptied. An empty side reads **Choose a front
cover photo** (or back); click it to open the same photo dialog as a page, titled **Choose
the front cover photo**, with every analysed photo shown cropped to the cover's shape.
Any photo can go on the cover, including one already on a page: the cover uses a copy and
the pages are left alone. Pick one and press **Use on the front cover**, or double-click it.
You can also drag a photo onto either cover panel, from a page or from **Left out**; the panel
is outlined and the label reads **Use on front cover** (or back). The page keeps its photo.
A photo that would put a face in the wrap or too near the board's edge, cut a face, or print
too small for the panel is dimmed with the reason and cannot be picked. Click a cover photo
to change it, or to take it off with **Remove** at the bottom of the dialog. The crop button
in the top corner of a cover photo opens the same crop dialog as a page, and a crop
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

**Chapter titles.** The book is divided into chapters, and each chapter has a title and a
title spot on the opening where it starts. Neither is printed unless you add the title yourself
in Pixajoy. A chapter starts on an opening whose first photo, read left to right
and top to bottom, comes from an event no earlier page has shown. An event that first appears
part-way through an opening joins the chapter already running, so one spread never carries
two titles. The title is the place name when the book was made with **Places** on and that
place's name has already been looked up; titles never trigger a lookup of their own.
Otherwise it is the dates the chapter's photos were taken, in the camera's local time, for
example "12–14 Mar 2026", "28 Mar – 2 Apr 2026" or "30 Dec 2025 – 2 Jan 2026". A chapter
with no capture dates is called "Chapter 2", and so on. The cover carries no text.

Where the title spot goes is chosen in this order:

1. The text area of a layout drawn with one, unless a photo has been moved onto it.
2. The tallest clear strip across the top or bottom of either page of the opening, at least
   0.35 in tall. It stays inside the safe margin, keeps out of the fold, and is never more
   than 1 in tall.
3. The widest clear column beside the photos, at least 1.5 in wide. Portrait pairs leave
   one.
4. Otherwise a strip across the top of the opening's first page, on top of a photo. The
   chapter heading says so, so you can decide where to put the title, or leave it out.

The suggested type size is between 14 and 28 pt, to suit the space.

Export carries no titles: they are not drawn into the photo files, `manifest.json` holds no
text, and the **Export** sheet does not list them. To add one, open that page in Pixajoy's
editor, add a text box, type the title, and place it where the editor shows the title spot.

