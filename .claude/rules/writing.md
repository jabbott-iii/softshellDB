# Rules for writing

These rules cover everything written for people to read in this
repository: `docs/`, the READMEs, `AGENTS.md`, `CLAUDE.md`,
`CONTRIBUTING.md`, code comments and doc comments, user-facing text in
code (errors, log messages, help text), the feature summaries in
`docs/features.jsonc`, commit messages, and pull request descriptions.
`documentation.md` adds the rules for docs and doc comments, and
`commits.md` the rules for commit messages.

Apply them to text you write or edit. Don't rewrite existing files only to
make them conform; fix a passage when you are already changing it
(`AGENTS.md`, rule 9: the smallest complete change).

## English: spelling and word choice

- Use American English, never British English.
  - Spelling: -or not -our (behavior, color), -ize and -ization not -ise
    and -isation (optimize, normalization), -yze not -yse (analyze), -er
    not -re (center), "license" for both noun and verb, a single "l"
    before suffixes (canceled, labeled, modeling, signaled), "gray",
    "catalog", "judgment".
  - Words: "toward", "among", "while", "learned", and "percent", not
    "towards", "amongst", "whilst", "learnt", or "per cent".
  - A British spelling you notice outside the text you are changing is
    not yours to fix as part of another task. Mention it in your summary
    instead, so the user can decide. Leave identifiers, quoted names, and
    URLs exactly as they are.
- Spell names the way their owners do: Rust, Cargo, rustup, Clippy,
  rustfmt, crates.io, GitHub, GitHub Actions, Linux, macOS, Windows, and
  Apple Silicon. Spell the project's name softshellDB, with a lowercase
  "s" and a capital "DB".
- Watch these common errors: its/it's, their/there/they're, affect/effect,
  then/than, lose/loose, lead/led, principal/principle,
  complement/compliment, ensure/insure.
- Use the one-word form for a noun and the two-word form for a verb: "the
  setup" but "set up the server". The same goes for login/log in,
  backup/back up, checkout/check out, shutdown/shut down, and rollout/roll
  out.
- Hyphenate a compound modifier before a noun ("a zero-dependency engine",
  "an open-source tool") but not after it ("the engine has zero
  dependencies", "the tool is open source"). Don't hyphenate after an
  "-ly" adverb ("a fully synced file").
- Prefer plain words: "use" (not "utilize" or "leverage"), "to" (not "in
  order to"), "because" (not "due to the fact that"), "for example" (not
  "e.g."), "that is" (not "i.e."), "about" (not "approximately").
- Cut filler and hedges: "basically", "simply", "just", "very", "really",
  "actually", "quite". Don't call something "easy" or "simple"; the reader
  decides that.
- Don't use marketing language without evidence: "blazing fast",
  "seamless", "robust", "cutting-edge", "revolutionary". State the
  measurement instead.
- Use inclusive, literal terms: allowlist/denylist, primary/replica,
  placeholder (not "dummy"), "quick check" (not "sanity check"), "they" for
  a person whose pronouns you don't know.

## English: grammar and punctuation

- Write in the active voice and present tense: "The flush thread writes an
  SST", not "An SST will be written by the flush thread". Use the passive
  only when the actor is unknown or doesn't matter.
- Keep one idea per sentence, and split sentences longer than about 25
  words.
- Use the serial (Oxford) comma: "Linux, macOS, and Windows". It removes
  ambiguity in long technical lists. Most existing pages omit it; don't
  change them just to add one.
- Keep lists parallel: every item starts with the same part of speech.
  Items that are full sentences end with a period; fragments don't. Don't
  mix the two in one list.
- Make every pronoun's referent clear. Write "This change breaks the
  cache", not "This breaks the cache", when the previous sentence holds
  more than one noun.
- Dashes:
  - Use an em dash (—) with no spaces for a break in a sentence.
  - Use an en dash (–) for ranges: "2–4 MB", "Monday–Friday".
  - Use a hyphen (-) only inside compound words.
  - Use a colon, not a spaced hyphen, between a title and a subtitle.
- In American prose, periods and commas go inside closing quotation marks.
  When the exact characters matter (a command, a value, a key), use code
  formatting so the punctuation stays outside it.
- Contractions such as "it's", "don't", and "isn't" are fine. Avoid
  awkward ones like "it'll" or "should've".
- Headings use sentence case: "Why a failed sync stops all writes".

## English: numbers, dates, and units

- In prose, spell out zero through nine and use numerals from 10 up.
  Always use numerals for versions, measurements, percentages, sizes,
  counts in tables, and anything technical.
- Put a space between a number and its unit: "16 MB", "4 ms", "500 ms".
  Don't add a space before "%". Use the standard symbols: ns, µs, ms, s,
  KB, MB, GB.
- Use thousands separators in prose ("10,000 keys") but never in versions,
  years, ports, or code.
- Write versions with a "v" when they name a release ("v1.2.0"), and match
  the project's own tags.
- Dates in prose: "November 7, 2026". Dates in file names, data, and ADRs:
  ISO 8601 ("2026-11-07"), with a time zone offset when there is a time.

## Technical writing

### Audience and readability

The audience ranges from college students and junior engineers to senior
engineers. Write so a student can follow every sentence and a senior
engineer finds nothing vague or wrong. Make the text as readable as
possible without losing any of its meaning.

- Assume the reader can program and has used Git and a terminal, but
  doesn't know this project or its specialty (storage engines,
  transactions, search indexes, and so on). Don't explain programming
  basics such as variables, loops, or functions.
- Keep jargon to a minimum:
  - Use a plain phrase when it says the same thing just as exactly: "runs
    at the same time", not "is concurrent", unless the distinction
    matters.
  - Use a technical term only when it carries meaning that a plain phrase
    can't, or when the reader needs the term to search for it, read the
    docs, or use the tool.
- Explain each technical term a student might not know the first time it
  appears. A short clause or a few words in parentheses are usually
  enough: "a Bloom filter (a small bit array that says a key is definitely
  absent or possibly present, so most lookups for a missing key skip
  reading the file)". After that, use the term without explaining it
  again.
- Spell out every acronym on first use, followed by the acronym in
  parentheses: "log-structured merge tree (LSM tree)". Skip this only for
  universal ones: HTML, API, URL, CLI, OS, CPU.
- Never trade accuracy for simplicity:
  - Keep the exact number, the real limit, the correct term, and every
    condition or caveat that changes the meaning.
  - If a simpler sentence would be wrong or misleading, keep the precise
    wording and add an explanation next to it.
  - Don't remove details a senior engineer needs; explain them so everyone
    else can follow too.
- Explain why, not only what. When describing a design choice, say what
  problem it solves or what it trades away, so a reader new to the field
  can follow the reasoning.
- Avoid insider shorthand:
  - Slang: "footgun", "yak shave", "bikeshed", "dogfood", "LGTM".
  - Internal nicknames and private abbreviations.
  - Bare references to records or tasks. When citing "ADR-0011" or "M3.7",
    say in the same sentence what it decided or what it builds.
- Introduce one new idea at a time, building on what has already been
  explained. Give abstract ideas a concrete example. Use an analogy only
  if it is accurate, and add it next to the real explanation rather than
  in place of it.
- Don't make readers feel behind: leave out "obviously", "of course",
  "trivially", "as you know", and "it's well known that".
- Keep paragraphs short (about three to five sentences) and about one
  topic. Write headings that let a reader skim the page and find the part
  they need.

### Content

- Lead with the point. The first sentence of a document or section says
  what changed or what the reader gets; background comes after.
- Be specific and checkable: version numbers, commit hashes, links to the
  roadmap task, pull request, or ADR, and numbers with units. Say what
  hardware and OS a measurement came from.
- Keep built and planned work apart. Say plainly what works today and what
  doesn't yet, and never describe a planned crate, command, or feature in
  the present tense. Many of the commands in `AGENTS.md` are still
  planned.
- Write instructions in the imperative, one action per numbered step, with
  the expected result after the action.
- Use code formatting (backticks) for file names, paths, commands, flags,
  environment variables, config keys, crate, function, and type names, and
  literal values. Don't use it for product names or for emphasis.
- Put code in fenced blocks with a language tag (`bash`, `rust`, `text`,
  `jsonc`). Leave out shell prompts (`$`), and keep command output in a
  separate block.
- Use descriptive link text: "the [dependency policy](…)", never "click
  here" or a bare "here". Link the first mention of something, not every
  mention.
- Use tables to compare three or more attributes across items, with a
  header row and short cells. Use lists for sequences and sets; use prose
  for reasoning.
- Write alt text that says what an image shows and why it matters, without
  "image of" or "screenshot of". Give purely decorative images `alt=""`.

## Before finishing

1. Read the rendered result, not only the source: preview changed
   Markdown, and for changed doc comments run
   `cargo doc --no-deps -p <crate>` and read the item's page.
2. Proofread changed prose for spelling, names, version numbers, and
   dates, and confirm every claim against its source (the code,
   `docs/roadmap.md`, `docs/decisions.md`, or the command's real output).
3. Do the two-reader check. Reread the draft as a second-year computer
   science student: is any term undefined, or any step skipped? Then
   reread it as a senior engineer: is anything vague, oversimplified, or
   wrong? Fix both.
