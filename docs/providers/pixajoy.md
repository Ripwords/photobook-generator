# Pixajoy (Taopix) API reference

Pixajoy runs Taopix Online Designer. This file records the HTTP calls PhotobookGen uses to
create and fill a Pixajoy project without a browser. Pixajoy publishes no API. Everything
here was observed from Pixajoy's own website and editor in November 2026, then replayed from
a script and checked in Pixajoy's editor. Expect it to change without notice.

Nothing here places an order. Never call `/api/project/order` or anything that adds to the
cart.

## Verified on 2026-11-09

- Create a project, register it in the user's **My Projects** list, and rename it.
- Upload a photo and wait for Pixajoy to process it.
- Delete a page's default box, add new picture boxes at any position and size, resize an
  existing box, and fill each one with a photo at a given crop.
- Add a text box on an interior page. Its text, including `März <3 & >`, came back from a
  reload byte for byte. Its rendering in the editor was not checked.
- Read pages 10 and up through `project/pageData/<slice>`.

Each of these was rendered in Pixajoy's editor and the positions and crops matched.

## Not yet known

- Any `styledtext` code beyond the font, size, colour and alignment ones below.
- Reopening an existing project. `OnlineAPI.editProject` worked twice and then returned
  result 24, "Project user mismatch", for reasons not found. PhotobookGen therefore creates a
  new project and fills it in the same editor session, and never reopens one.
- Adding pages after creation. The page count is fixed when the project is created.
- `rotation` on a box. Uploading pixels that are already upright avoids it.

## Transport

`service.pixajoy.com` speaks only TLS 1.2 with CBC cipher suites. rustls offers no CBC suite,
so its handshake is reset. PhotobookGen builds its Pixajoy client on native-tls.

## Units

Every length is inches × 10¹⁴, sent and returned as a decimal string
(`"1122047244094486"` is 11.22047244094486 in). `boxscrollleft` and `boxscrolltop` are plain
numbers in inches.

## 1. Sign in

The user signs in on `https://www.pixajoy.com.my/dashboard/projects`, which shows a login form when signed out. There is no login page: `/login` and similar paths render the site's 404. Google, Apple and Facebook sign in through a `window.open` popup
that posts back to its opener. PhotobookGen opens each popup in a window that shares the
login window's WebKit configuration, so the popup sees the same private cookies. The site stores an RS256 JWT in the
`mbtk` cookie on `www.pixajoy.com.my`. Its payload carries `emailaddress`, `userid` and
`exp`, and it lasts about 30 days. Every call to `service.pixajoy.com` sends it as
`Authorization: Bearer <mbtk>`.

## 2. Create the project

1. `GET https://service.pixajoy.com/api/token/GetTaopixCartLinkP` returns
   `{ link: "r=<guid>&u=<userid>&s=<sign>" }`.
2. `POST https://cart.pixajoy.com.my/?fsaction=OnlineAPI.createProject2&dd=<dd>&<link>&cpexpo=WIZARD:NDA|EDITOR:4QNEPKQEVVXF`
   - `dd` is a device-detection string. The value the website sends works:
     `v1s19.2o154.28571428571428o384o210o3d0o0o1o771.4857142857143`.
   - A browser `User-Agent` is required. Without one the response is result 12, "Your
     browser is not supported".
   - The body is form-encoded: `brandcode=PIXAJOY&groupcode=PIXAJOY&collectioncode=HNS_BLK02&productcode=IHL8511&pagestyle=&projectname=<name>&txtfield=defaultpages=<n>`.
   - The response is `{ result: 0, projectref, projectname, collectionname, designurl }`.
3. `POST https://service.pixajoy.com/api/project/insert` with JSON
   `{ webbrandcode, groupcode, collectioncode, collectionname, productcode, productname, projectref, projectname, thumbnail, componentlist: "", salesmethod: 1 }`.
   Without this call, the project does not appear in **My Projects**.

`HNS_BLK02` / `IHL8511` is "Photo Book Blank, Fit to Page", Imagewrap Hardcover 8.5" × 11"
Landscape. It allows 20 to 150 pages.

## 3. Open the editor session

`designurl`'s `ref` query parameter is base64 of `?ref=<sessionref>&theme=...`.

`POST https://editor.pixajoy.com.my/api/authenticate` as multipart, with these fields:

- `pubkey`: the brand key from `https://editor.pixajoy.com.my/ui/pixajoy/config.js`, which
  every browser receives.
- `ref`: the session ref.
- `designerId`: a random UUID.

It returns `{ token, refreshToken }`. **The session ref works once.** A second authenticate
with the same ref returns 400, so the whole fill must happen in this session.

Every editor call sends `Authorization: Bearer <token>`. Call `GET system/initialise`,
then `GET project/initialise`, then `GET project/load`. A token is refreshed with
`POST /api/reauthenticate`, body `{}`, bearer `<refreshToken>`.

## 4. What `project/load` returns

- `layout` has `width` and `height` for one page including bleed, plus `bleed`,
  `gutterwidth`, `paperwidth` and `paperheight`.
  - For IHL8511 the page is 11.2205 × 8.7402 in (285 × 222 mm).
  - Bleed is 0.19685 in (5 mm) and the gutter 0.70866 in (18 mm).
  - Read these values from the response rather than hard-coding them.
- `project` is keyed by page-record id. Each record has `pagenumber` and an `items` map
  keyed by box ref (an empty array when there are none).
- Interior boxes use one horizontal strip. The box for page `n` starts at
  `boxleft = n × layout.width`. Page 1 is a single right-hand page, then 2–3, 4–5 and so on.
  The last page is a single left-hand page.
- A new blank project has one full-page picture box (`type: 0`) on every interior page.
- The cover is `pagenumber: 100000`. Its picture boxes in this template are the front photo,
  8.64 × 6.48 in at (13.68, 1.40), and a 3.09 in square on the back. It also has text boxes
  (`type: 1`), several with `protectfromdelete: true`. One has `protectfromdelete: false` and
  `labelshort` "Insert Your Text Here". PhotobookGen copies that one for chapter titles.
- **Pages load lazily.** `load` fills `items` only for records with `pagenumber < 10`. Later
  records have `itemstate: 0` and `items: []`. `GET project/pageData/<slice>`, where
  `slice = floor(pagenumber / 10)`, returns `{ pageItems: { "<record id>": { "<ref>": {...} } } }`
  with `pageItems` at the top level of the body. A slice with no boxes left returns
  `pageItems: []`, PHP's empty map.

## 5. Upload a photo

1. `POST /api/assets/create` with
   `{ assets: [{ fileSizeBytes, name, fileType: 1, uploadid, width, height, batchRef, colourComponent: 0, colourMode: 3, fileDateModified, rotation: 0, assetType: 106, metaData: { dateTimeOriginal: "0000-00-00 00:00:00", latitude: "", longitude: "" }, status: 0, aithumb: null, thumbnail: false }] }`.
   - `uploadid` is any 21 characters.
   - The response is `[{ id, volumeId, fileName, folderPath, signature }]`.
2. `POST https://images.pixajoy.com.my{folderPath}/{fileName}{signature}` as multipart, with
   the file in the field `asset`.
3. `POST /api/assets/queue` with
   `{ asset: { assetId, jobType: "1", uploadDuration, fileSizeBytes, vid: volumeId, volumeType: 1, assetType: 106, scaled: false }, queue: "process" }`.
4. Poll `POST /api/assets/status` with `{ assets: [id] }`.
   - Status 6 means processed. Status 7 or 8 means it failed.

## 6. Save

`POST /api/project/save` takes a diff:

```json
{
  "pages":  { "deleted": {}, "added": {}, "changed": {} },
  "items":  { "deleted": { "<ref>": <ref> }, "added": { "-1": { ... } }, "changed": { "<ref>": { ... } } },
  "events": { "deleted": {}, "added": {}, "changed": {} },
  "itemsToPages": {}, "projectName": "<optional rename>", "autoSave": false
}
```

The diff takes these operations:

- **Delete a box.** Add `"<ref>": <ref>` to `items.deleted`.
- **Add a box.** Copy an existing box object and drop `ref`, `recordid` and `boxtype`. Give it
  a negative `ref`, and key it by that ref in `items.added`. The response's `pageItems` maps
  each `oldRef` to its `newRef`.
- **Move or resize a box.** Send `boxleft`, `boxtop`, `boxwidth` and `boxheight` in
  `items.changed`. Send `boxlinex1`, `boxliney1`, `boxlinex2` and `boxliney2` as the same
  rectangle.
- **Fill a box with a photo.** Send these fields: `hiresfiletype: 106`,
  `hirespath: "<asset id>"`, `hireswidth`, `hiresheight`, `rotatedhireswidth`,
  `rotatedhiresheight`, `magnification`, `boxscrollleft` and `boxscrolltop`.
- **Add a text box.** Copy the cover's unprotected text box as for a picture box. Set
  `pagenumber` and the geometry, `backcolour: -1` (transparent), `labelshort: {}`, and a
  `zindex` above the page's photos. Keep `styledtext` and replace only its `data`:

  ```
  <fi0*0400*ARIALMT\tArialMT\tArial\tArial\tArial (TT)><fn0*0400*ARIALMT><ts{pt × 100}><tc{rgb}><pa{align}>{len} <..>{text}
  ```

  `\t` is a tab. `tc` is the colour as one integer, `0xRRGGBB`. `pa` is 0 left, 1 centre,
  2 right. `len` is the text's length in UTF-16 code units, so "🗻" counts 2. The text is
  sent raw; `<`, `>` and `&` need no escaping.
- **Rename the project.** Set `projectName`.

## Crop maths

For a photo of `pw × ph` pixels in a box of `bw × bh` inches:

- `fit = min(bw/pw, bh/ph)` is the scale that shows the whole photo. That scale is
  magnification 100.
- `fill = max(bw/pw, bh/ph)` is the scale that covers the box.
- `magnification = 100 × fill / fit`.
- `boxscrollleft = −(pw × fill − bw) × ax` and `boxscrolltop = −(ph × fill − bh) × ay`.
  `ax` and `ay` run from 0 for the left or top edge to 0.5 for centred.

Check case: a 1200 × 800 photo in an 11.2205 × 8.7402 in box gives magnification 116.842 and
a centred `boxscrollleft` of −0.9449. Pixajoy's editor drew it centred.
