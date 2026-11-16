# Local time anywhere (/docs/tutorials/local-time)

A calendar, a prayer-time or sunrise app, a booking form: each needs the local time *at a
place*, not on the device. teigeodata answers it from the pack alone: the zone boundaries say
which zone a point is in, and the tzdb rules carried in the pack say its offset at any instant,
so an old phone with stale rules still gets it right.

<Steps>
<Step>
### The zone at a point [#the-zone-at-a-point]

`tz_at` gives the IANA zone the point falls in, as the zone boundaries draw it. Every place
also carries its zone in its `tz` column, so a place picked from search needs no second query.
</Step>

<Step>
### The offset at an instant [#the-offset-at-an-instant]

With the zone, `tz_offset_at` gives the UTC offset at a Unix time: seconds east of UTC, the
abbreviation, and whether daylight saving is in force. Nepal is `+0545` all year; New York
changes twice a year.
</Step>

<Step>
### Check that a local time exists [#check-that-a-local-time-exists]

When a user picks a wall-clock time, `tz_local` says whether it happens once (`unique`), never
(`gap`: the clocks skipped it) or twice (`overlap`: the clocks went back), with the offsets
before and after. A booking form should refuse the gap and ask which of the two in an overlap.

The three steps, in each language:

<TourCode region="timezone" />
</Step>

<Step>
### Know how fresh the rules are [#know-how-fresh-the-rules-are]

`tz_version` names the tzdb release the rules come from (`2026f` above). Governments change
their rules with short notice; a newer data release carries the new rules, and the
[store](/docs/concepts/store) updates installed packs.
</Step>
</Steps>

## Next [#next]

- [Timezones](/docs/guides/timezones) in full.
- [Elevation](/docs/guides/elevation): with the height of the point, sunrise and sunset times
  can account for the horizon dip.