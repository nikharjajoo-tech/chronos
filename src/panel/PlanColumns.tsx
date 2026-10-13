import { useState, type ReactNode } from "react";
import { fmtDuration } from "../format";
import type { Snapshot, TaskRow } from "../types";
import { BOARD_BUCKETS, COLUMN_LABEL, PANEL_COLUMNS, doneSince, openIn, projectColor, spentOf, startOfDay, type Column, type Load } from "./planModel";
import { DoneCard, TaskCard, type CardActions } from "./TaskCard";
import type { useDragList } from "./useDragList";

type Dnd = ReturnType<typeof useDragList>;

interface Props {
  tasks: TaskRow[];
  snap: Snapshot | null;
  now: number;
  actions: CardActions;
  dnd: Dnd;
}

const EMPTY: Record<Column, string> = {
  today: "Nothing planned for today. Add a task above, or bring one in from This week.",
  week: "Nothing for this week yet.",
  backlog: "The backlog is empty. Park ideas here for later.",
  done: "Nothing finished yet today.",
};

/** Panel layout: one column at a time, picked with a segmented switcher. */
export function PanelColumns({ tasks, column, setColumn, snap, now, actions, dnd }: Props & { column: Column; setColumn(c: Column): void }) {
  const today = startOfDay(now);
  const doneToday = doneSince(tasks, today);
  const count = (c: Column) => (c === "done" ? doneToday.length : openIn(tasks, c).length);
  const over = dnd.drag?.over && !dnd.drag.over.list ? dnd.drag.over.column : null;

  return (
    <>
      <div className="plan-cols" role="tablist" aria-label="Lists">
        {PANEL_COLUMNS.map((c) => (
          <button
            key={c}
            role="tab"
            aria-selected={column === c}
            className={`${column === c ? "on" : ""}${over === c ? " drop-over" : ""}`}
            data-drop-column={c}
            onClick={() => setColumn(c)}
          >
            {COLUMN_LABEL[c]} <span className="count">{count(c)}</span>
          </button>
        ))}
      </div>

      {column === "done" ? (
        <DoneColumn tasks={tasks} today={today} actions={actions} />
      ) : (
        <>
          <ul className={listClass(dnd)} data-drop-list={column}>
            <Cards column={column} tasks={openIn(tasks, column)} snap={snap} now={now} actions={actions} dnd={dnd} />
          </ul>
          {column === "today" && doneToday.length > 0 && (
            <section className="done-group">
              <h4>
                Done today {doneMs(doneToday) > 0 && <span className="count">· {fmtDuration(doneMs(doneToday))}</span>}
              </h4>
              <ul className="task-list quiet">
                {doneToday.map((t) => (
                  <DoneCard key={t.id} task={t} onReopen={() => actions.setDone(t, false)} />
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </>
  );
}

/** Board layout: Backlog · This week · Today · Done side by side; drag across them. */
export function BoardColumns({ tasks, snap, now, actions, dnd, load, liveNote }: Props & { load: Load; liveNote?: string }) {
  const today = startOfDay(now);
  const doneTodayMs = doneMs(doneSince(tasks, today));
  const drag = dnd.drag;
  const over = drag?.over?.column ?? null;

  const column = (c: Column, side: ReactNode, body: ReactNode, count: number) => {
    // A drop target from another column gets room for the card at its end.
    const target = drag && over === c && drag.from !== c;
    return (
      <section
        key={c}
        className={`board-col ${c}${target ? " drop-over" : ""}`}
        aria-label={COLUMN_LABEL[c]}
        data-drop-list={c}
        style={target && c !== "done" ? { paddingBottom: 10 + drag.size } : undefined}
      >
        <header>
          <h2>
            {COLUMN_LABEL[c]} <span className="count">{count}</span>
          </h2>
          {side && <span>{side}</span>}
        </header>
        {body}
      </section>
    );
  };

  return (
    <div className="board-cols">
      {BOARD_BUCKETS.map((b) => {
        const list = openIn(tasks, b);
        return column(
          b,
          b === "today" && (
            <span className={load.goalMs > 0 && load.projectedMs > load.goalMs ? "over" : undefined}>
              {load.goalMs > 0 ? `${fmtDuration(load.projectedMs)} of ${fmtDuration(load.goalMs)}` : fmtDuration(load.projectedMs)}
            </span>
          ),
          list.length > 0 ? (
            <ul className={listClass(dnd, "cards")}>
              <Cards column={b} tasks={list} snap={snap} now={now} actions={actions} dnd={dnd} board liveNote={liveNote} />
            </ul>
          ) : (
            <p className="col-empty">{drag ? "Drop here" : b === "today" ? "Drag tasks here, or add one above." : "Nothing here yet."}</p>
          ),
          list.length,
        );
      })}
      {column("done", doneTodayMs > 0 ? `${fmtDuration(doneTodayMs)} today` : "today", <DoneColumn tasks={tasks} today={today} actions={actions} cards />, doneSince(tasks, today).length)}
    </div>
  );
}

/** Focus spent on a list of finished tasks. */
const doneMs = (done: TaskRow[]) => done.reduce((a, t) => a + t.spentMs, 0);

function listClass(dnd: Dnd, extra = "") {
  return `task-list${extra ? ` ${extra}` : ""}${dnd.settling ? " settling" : ""}${dnd.drag ? " sorting" : ""}`;
}

function Cards({ column, tasks, snap, now, actions, dnd, board, liveNote }: Props & { column: Column; board?: boolean; liveNote?: string }) {
  const ids = tasks.map((t) => t.id);
  if (tasks.length === 0) return <li className="list-empty">{EMPTY[column]}</li>;
  return tasks.map((t) => (
    <TaskCard
      key={t.id}
      task={t}
      live={snap?.taskId === t.id}
      spentMs={spentOf(t, snap, now)}
      now={now}
      actions={actions}
      dragging={dnd.drag?.id === t.id}
      board={board}
      note={liveNote}
      style={dnd.styleFor(t.id, column, ids)}
      onPointerDown={(e) => dnd.start(e, t.id, column)}
      consumeClick={dnd.consumeClick}
    />
  ));
}

function DoneColumn({ tasks, today, actions, cards }: { tasks: TaskRow[]; today: number; actions: CardActions; cards?: boolean }) {
  const [open, setOpen] = useState(false);
  const doneToday = doneSince(tasks, today);
  const earlier = doneSince(tasks, today - 6 * 86_400_000, today);
  const cls = `task-list${cards ? " cards" : ""}`;
  return (
    <>
      {doneToday.length > 0 ? (
        <ul className={cls} data-drop-list={cards ? undefined : "done"}>
          {doneToday.map((t) => (
            <DoneCard key={t.id} task={t} onReopen={() => actions.setDone(t, false)} />
          ))}
        </ul>
      ) : cards ? (
        <p className="col-empty">Tick a task off, or drag it here.</p>
      ) : (
        <ul className="task-list empty-list" data-drop-list="done">
          <li className="list-empty">{EMPTY.done}</li>
        </ul>
      )}
      {earlier.length > 0 && (
        <section className="done-group">
          <button className="group-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
            <span className={`chev${open ? " open" : ""}`} aria-hidden>›</span> Earlier this week <span className="count">{earlier.length}</span>
          </button>
          {open && (
            <ul className={`${cls} quiet`}>
              {earlier.map((t) => (
                <DoneCard key={t.id} task={t} onReopen={() => actions.setDone(t, false)} />
              ))}
            </ul>
          )}
        </section>
      )}
    </>
  );
}

/** Board sidebar: filter by project, and Today's load. */
export function BoardSidebar({ tasks, filter, setFilter, load }: { tasks: TaskRow[]; filter: string | null; setFilter(p: string | null): void; load: Load }) {
  const open = tasks.filter((t) => !t.done);
  const counts = new Map<string, number>();
  for (const t of open) if (t.project) counts.set(t.project, (counts.get(t.project) ?? 0) + 1);
  const projects = [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
  const over = load.goalMs > 0 ? load.projectedMs - load.goalMs : 0;

  return (
    <aside className="board-side">
      <nav aria-label="Projects">
        <span className="side-label">Projects</span>
        <button className={filter === null ? "on" : ""} onClick={() => setFilter(null)}>
          <span>All tasks</span> <span className="count">{open.length}</span>
        </button>
        {projects.map(([p, n]) => (
          <button key={p} className={filter === p ? "on" : ""} onClick={() => setFilter(filter === p ? null : p)}>
            <span className="side-project">
              <i style={{ background: projectColor(p) }} />#{p}
            </span>
            <span className="count">{n}</span>
          </button>
        ))}
        {filter !== null && !counts.has(filter) && (
          <button className="on" onClick={() => setFilter(null)}>
            <span>#{filter}</span> <span className="count">0</span>
          </button>
        )}
      </nav>
      <span className="plan-spacer" />
      <div className="load-card">
        <span className="side-label">Today’s load</span>
        <span className="load-big">
          {fmtDuration(load.projectedMs)} {load.goalMs > 0 && <small>of {fmtDuration(load.goalMs)}</small>}
        </span>
        <LoadMeter load={load} />
        <span className="load-note">
          {over > 0 ? `${fmtDuration(over)} more than your goal.` : over < 0 ? `${fmtDuration(-over)} to spare before your goal.` : load.goalMs > 0 ? "Right on your goal." : ""}
          {load.unestimated > 0 && ` ${load.unestimated} ${load.unestimated === 1 ? "task has" : "tasks have"} no estimate.`}
        </span>
      </div>
    </aside>
  );
}

/** Done (solid), still to go (light) and anything past the goal (striped). */
export function LoadMeter({ load }: { load: Load }) {
  const { doneMs, toGoMs, projectedMs, goalMs } = load;
  const scale = Math.max(goalMs, projectedMs, 1);
  const overMs = goalMs > 0 ? projectedMs - goalMs : 0;
  const pct = (ms: number) => `${(ms / scale) * 100}%`;
  return (
    <div className="load-bar" aria-hidden>
      <i className="done" style={{ width: pct(doneMs) }} />
      <i className="togo" style={{ left: pct(doneMs), width: pct(toGoMs) }} />
      {overMs > 0 && <i className="over" style={{ left: pct(goalMs), width: pct(overMs) }} />}
    </div>
  );
}
