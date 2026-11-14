"""Awaiting a job from ``asyncio`` (bindings §6).

A job's calls release the GIL, so they run on worker threads without holding up the event
loop. These helpers add what ``asyncio.to_thread`` alone lacks: cancelling the awaiting task
cancels the job, and progress arrives as an async iterator.
"""

from __future__ import annotations

import asyncio
import contextlib
from collections.abc import AsyncIterator

from teigeodata._native import Job

__all__ = ["progress", "result"]


async def result(job: Job) -> bytes:
    """``job.result()`` on a worker thread.

    Cancelling the awaiting task cancels the job; the cancellation is raised once the job has
    ended, so nothing of it runs on afterwards.
    """
    work = asyncio.ensure_future(asyncio.to_thread(job.result))
    try:
        return await asyncio.shield(work)
    except asyncio.CancelledError:
        job.cancel()
        with contextlib.suppress(Exception):
            await work
        raise


async def progress(job: Job, poll_ms: int = 250) -> AsyncIterator[tuple[int, int]]:
    """The job's progress, ``(done, total)`` bytes: now, then at each change, ending when the
    job ends (the last at its total). Leaving the loop stops the waiting, not the job."""
    seq = job.seq()
    last = (job.done_bytes(), job.total_bytes())
    yield last
    while not job.is_ended():
        nxt = await asyncio.to_thread(job.wait_progress, seq, poll_ms)
        if nxt != seq:
            seq = nxt
            last = (job.done_bytes(), job.total_bytes())
            yield last
    end = (job.done_bytes(), job.total_bytes())
    if end != last:
        yield end
