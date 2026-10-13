import { useState } from "react";
import type { Snapshot, TaskRow } from "../types";
import { COLUMN_LABEL, PANEL_COLUMNS, doneSince, openIn, spentOf, startOfDay, type Column } from "./planModel";
import { DoneCard, TaskCard, type CardActions } from "./TaskCard";
import type { useDragList } from "./useDragList";

interface Props {
  tasks: TaskRow[];
  column: Column;
  setColumn(c: Column): void;
  snap: Snapshot | null;
  now: number;
  actions: CardActions;
  dnd: ReturnType<typeof useDragList>;
}

const EMPTY: Record<Column, string> = {
  today: "Nothing planned for today. Add a task above, or bring one in from This week.",
  week: "Nothing for this week yet.",
  backlog: "The backlog is empty. Park ideas here for later.",
  done: "Nothing finished yet today.",
};

/** Panel layout: one column at a time, picked with a segmented switcher. */
export function PanelColumns({ tasks, column, setColumn, snap, now, actions, dnd }: Props) {
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
          <TaskList column={column} tasks={openIn(tasks, column)} snap={snap} now={now} actions={actions} dnd={dnd} />
          {column === "today" && doneToday.length > 0 && (
            <section className="done-group">
              <h4>Done today</h4>
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

export function TaskList({ column, tasks, snap, now, actions, dnd }: { column: Column; tasks: TaskRow[]; snap: Snapshot | null; now: number; actions: CardActions; dnd: ReturnType<typeof useDragList> }) {
  const ids = tasks.map((t) => t.id);
  if (tasks.length === 0) {
    return (
      <ul className="task-list empty-list" data-drop-list={column}>
        <li className="list-empty">{EMPTY[column]}</li>
      </ul>
    );
  }
  return (
    <ul className={`task-list${dnd.settling ? " settling" : ""}${dnd.drag ? " sorting" : ""}`} data-drop-list={column}>
      {tasks.map((t) => (
        <TaskCard
          key={t.id}
          task={t}
          live={snap?.taskId === t.id}
          spentMs={spentOf(t, snap, now)}
          now={now}
          actions={actions}
          dragging={dnd.drag?.id === t.id}
          style={dnd.styleFor(t.id, column, ids)}
          onPointerDown={(e) => dnd.start(e, t.id, column)}
          consumeClick={dnd.consumeClick}
        />
      ))}
    </ul>
  );
}

function DoneColumn({ tasks, today, actions }: { tasks: TaskRow[]; today: number; actions: CardActions }) {
  const [open, setOpen] = useState(false);
  const doneToday = doneSince(tasks, today);
  const earlier = doneSince(tasks, today - 6 * 86_400_000, today);
  return (
    <>
      {doneToday.length > 0 ? (
        <ul className="task-list" data-drop-list="done">
          {doneToday.map((t) => (
            <DoneCard key={t.id} task={t} onReopen={() => actions.setDone(t, false)} />
          ))}
        </ul>
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
            <ul className="task-list quiet">
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
