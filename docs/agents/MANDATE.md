# Original human mandate (verbatim, 2026-09-11)

Read only if `docs/agents/PROTOCOL.md` is ambiguous. PROTOCOL.md is the operational form of this text; if they conflict, this text wins and PROTOCOL.md must be fixed.

---

Your role for this current prompt is to come up with a ridiculously strong, efficient, very lightweight system for all agents, as well as me and my teammate, to collaborate on this repo. We will be using models from grok, claude, and openai. I want to ensure that, no matter what agent is working on the repo, they will maintain complete context of all tasks executed prior, reasoning, etc, and that no agent will ever be biased towards any particular implementation because of this reasoning trail. The trail must be setup such that all agents working on the repo are directed to view it ONLY WHEN NECESSARY (we do not want to burn tokens for no good reason), and that no matter if changes are pushed/pulled from my computer or my teammates', both of our trails will merge sufficiently well regardless. It is incredibly important that I should literally be able to stop a task midway using one agent, and enough context will be stored about the task such that I can literally give the whole prompt to another agent / another model provider in a fresh session and it will pickup right where the original agent left-off. VERY IMPORTANT: DO NOT over-write or waste ridiculous volumes of tokens just documenting/writing. Be reasonable and hyper-efficient and lightweight.

Here is an EXTREMELY important claude-specific instruction: ALL subagents being dispersed MUST utilize Opus 5. We will EXCLUSIVELY utilize Fable for rigorous planning and orchestration, and EXCLUSIVELY Opus for execution. Fable must be incredibly precise, detailed, and granular in the prompts and instructions that it gives to Opus. Ensure this is added to CLAUDE.md.

My team-mate also is working on my terminal through a remote connection, so it is possible we have multiple agent sessions manipulating different parts of the directory at the same time. Do not freak out if this is ever the case. Most of the time, his work will be confined to the 'backend' directory, and mine to the 'client' directory.

Ensure all the agent/model-provider-specific first-point-of-contact files have content that directs them to the relevant subfiles containing all of the aforementioned instructions pertaining to context and task tracking, vision, etc.

ALL INSTRUCTIONS BEING WRITTEN HERE MUST BE REMEMBERED AND STORED IN PERPETUITY AND SHOULD ALWAYS BE ACCESSIBLE BY AGENTS WHEN REQUIRED OF THEM.

ALL actual instructions for execution will be given later, your goal right now is simply to prime the repo so that it is perfect for human-agent collaboration in all circumstances.
