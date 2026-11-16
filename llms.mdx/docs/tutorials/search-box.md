# A place search box (/docs/tutorials/search-box)

A user types into a box; under it, the right places appear as they type. This walkthrough
builds that box's engine side: one engine for the app, a query per keystroke, near places
first, and each row labelled with the units that hold it.

<Steps>
<Step>
### Open the pack once [#open-the-pack-once]

Open the engine when the app starts and keep it: opening maps the file and checks its head,
and each section is verified the first time a query reads it. One engine answers every
thread, so the search box and the rest of the app share it.

<TourCode region="open" />
</Step>

<Step>
### Search on each keystroke [#search-on-each-keystroke]

Pass what the user has typed so far. Search matches every name of every place, in every
script, and forgives typos, missing accents and abbreviations, so the box needs no
normalisation of its own. Ask for the few rows the box shows.

<TourCode region="search" />

On a low-end phone a keystroke is estimated at 1.3 ms at the 99th percentile
([measured](/docs/data/measured#phone-class-estimates)): fast enough to run on every keystroke
without a debounce, though off the UI thread is still the right place for it.
</Step>

<Step>
### Rank the places near the user first [#rank-the-places-near-the-user-first]

Many places share a name. Pass the user's location (or the middle of the map they are looking
at) as the **focus**: places near it rank first, the gain halving with every `focus_scale_m`
of distance.

<TourCode region="search-focus" />

For an autocomplete where an exact name must always be on top, set `strict`: rows rank by how
well the name matched (exact, loose, prefix, one edit, substring, two edits, phonetic), then
by importance and focus.
</Step>

<Step>
### Let users type the parent [#let-users-type-the-parent]

People write "Kathmandu, Bagmati" to mean the one in Bagmati. Search reads the part after the
comma as an ancestor of any level and ranks places inside it first; the box passes the text as
it is.

<TourCode region="search-parent" />
</Step>

<Step>
### Label each row [#label-each-row]

Two rows called "Pokhara" need their district and province to tell them apart. The ancestors
of a place give the label, parent first:

<TourCode region="hierarchy" />

Show the name and the first one or two ancestors ("Pokhara · Kaski, Gandaki"). When the user
picks a row, store its `id`: IDs survive data updates, and a merged place's old ID answers
with the survivor.
</Step>
</Steps>

## Next [#next]

- [Search](/docs/guides/search) in full: every option and column.
- [A GPS fix to a place](/docs/tutorials/gps-to-place): the reverse of this box.