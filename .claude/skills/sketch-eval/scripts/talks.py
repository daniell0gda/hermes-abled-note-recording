"""Test talks for the live sketch: fluent spoken explanations, split the way STT cuts them, with the facts a good
diagram must show. Facts are concept regexes (matched on node label and id) plus edges between concepts.

`evaluate(talk, view)` returns (points, total, notes): concept nodes, direct edges, paths, lanes, then generic
checks (node budget, no orphans, no duplicate concepts, labelled edges, acceptable kind).
"""
import re
from dataclasses import dataclass, field

LABELLED_EDGES = 0.5


@dataclass(frozen=True)
class Talk:
    name: str
    kinds: tuple[str, ...]
    segments: tuple[str, ...]
    concepts: dict[str, str]
    edges: tuple[tuple[str, str], ...] = ()
    paths: tuple[tuple[str, str], ...] = ()
    lanes: dict[str, str] = field(default_factory=dict)
    max_nodes: int = 12
    unique: tuple[str, ...] = ()


TALKS = {talk.name: talk for talk in (
    Talk(
        "issue", ("flow", "swimlane", "layers"),
        ("You start by calling a create-issue skill, which opens an issue on GitHub.",
         "Then you call a start-issue skill, which runs a workflow owned by a team leader.",
         "That leader manages a small team: a planner, an implementor, and a checker.",
         "The planner drafts the plan, the implementor builds against it, and the checker reviews the result;",
         "if the checker is not satisfied, work goes back to the implementor until it is.",
         "A manual tester only steps in when something needs a visual check.",
         "When the work is good, the team leader reviews the artifacts for each team member (plan leaves the plan.md, "
         "implementation notes, checker findings), and the issue is done."),
        {"create issue": r"create.?issue", "github issue": r"github|^issue$", "start issue": r"start.?issue",
         "leader": r"leader|review.*artifact|run.*workflow", "planner": r"planner|draft.*plan|^plan$",
         "implementor": r"implement(or|er)\b|^build", "checker": r"checker(?!.*finding)|review.*result|^check\b",
         "tester": r"manual|tester|visual", "artifacts": r"artifact|plan\.md|notes|finding",
         "done": r"done|closed|complete|github|^issue$"},
        edges=(("planner", "implementor"), ("implementor", "checker"), ("checker", "implementor"),
               ("checker", "tester")),
        paths=(("create issue", "start issue"), ("start issue", "leader"), ("checker", "leader"), ("leader", "done")),
        max_nodes=13, unique=("planner", "implementor", "start issue"),
    ),
    Talk(
        "login", ("sequence",),
        ("Okay so let me explain how the login with Google works, because people keep asking me about it.",
         "So the user is on our web app and they click sign in with Google.",
         "The browser then gets redirected to Google's login page, right,",
         "and the user types their password there, not on our site, that's kind of the whole point.",
         "Google then sends the browser back to our callback URL with a code.",
         "Our backend takes that code and exchanges it with Google for a token. That's server to server, the browser "
         "never sees that part.",
         "Then the backend, hmm, it creates a session for the user and sets a cookie on the browser.",
         "And from then on every request from the browser just carries that cookie."),
        {"browser": r"browser|user|web app|client", "google": r"google", "backend": r"backend|server|api"},
        edges=(("browser", "google"), ("google", "browser"), ("backend", "google"), ("google", "backend"),
               ("backend", "browser")),
        max_nodes=5,
    ),
    Talk(
        "order", ("state",),
        ("Let me tell you about the life of an order in our shop, it's simpler than it looks.",
         "When someone puts stuff in the cart and checks out, the order is created as pending.",
         "It stays pending until the payment goes through, and then it's paid.",
         "Oh, and if the payment fails three times, well, then we just cancel it.",
         "Once it's paid the warehouse picks it up and it becomes shipped,",
         "and when the courier confirms, it's delivered. That's the happy path.",
         "From delivered the customer still has two weeks to send it back, then it's returned and we refund them.",
         "Actually, wait, you can also cancel while it's still paid, before it ships. So paid can go to cancelled too."),
        {"pending": r"pending", "paid": r"paid", "cancelled": r"cancel", "shipped": r"shipped",
         "delivered": r"delivered", "returned": r"return|refund"},
        edges=(("pending", "paid"), ("pending", "cancelled"), ("paid", "shipped"), ("shipped", "delivered"),
               ("delivered", "returned"), ("paid", "cancelled")),
        max_nodes=8, unique=("paid", "shipped", "delivered"),
    ),
    Talk(
        "org", ("tree",),
        ("So you asked how the engineering department is organized, right?",
         "At the top there's Maria, she's the head of engineering.",
         "Under her there are three groups, platform, product and data.",
         "Platform is led by Tom, and he has the infra team and the developer tools team.",
         "Product is split by app, so we've got the web team and the mobile team, both under Anna.",
         "And data, um, that's just one team for now, the analytics team, under Raj.",
         "Oh, I forgot, there's also a security team, and it reports directly to Maria, not to any of the groups."),
        {"maria": r"maria|head", "platform": r"platform|tom", "product": r"product|anna", "data": r"\bdata\b|raj",
         "infra": r"infra", "devtools": r"tool", "web": r"\bweb\b", "mobile": r"mobile", "analytics": r"analytic",
         "security": r"security"},
        edges=(("platform", "infra"), ("platform", "devtools"), ("product", "web"), ("product", "mobile"),
               ("data", "analytics"), ("maria", "security")),
        paths=(("maria", "platform"), ("maria", "product"), ("maria", "data")),
        max_nodes=13,
    ),
    Talk(
        "backend", ("flow", "dependency", "layers"),
        ("Okay, so here's roughly how our backend looks, at least the important parts.",
         "Everything comes in through the mobile app, which talks to the API gateway.",
         "The gateway forwards requests to two services, users and orders.",
         "Both of them have their own Postgres database, they don't share one, that was a big decision for us.",
         "The orders service, hmm, when an order is placed it puts a message on a Kafka topic.",
         "And there's a worker that reads from Kafka and sends the confirmation emails through SendGrid.",
         "Oh and the users service caches profiles in Redis, because the app asks for them all the time."),
        {"app": r"mobile|app\b", "gateway": r"gateway", "users": r"^users?\b(?!.*(db|database|postgres))|user.?service",
         "orders": r"^orders?\b(?!.*(db|database|postgres))|order.?service", "users db": r"user.*(db|postgres|database)",
         "orders db": r"order.*(db|postgres|database)", "kafka": r"kafka|topic", "worker": r"worker",
         "sendgrid": r"sendgrid|email", "redis": r"redis|cache"},
        edges=(("app", "gateway"), ("gateway", "users"), ("gateway", "orders"), ("users", "users db"),
               ("orders", "orders db"), ("orders", "kafka"), ("kafka", "worker"), ("worker", "sendgrid"),
               ("users", "redis")),
        max_nodes=12,
    ),
    Talk(
        "hiring", ("swimlane",),
        ("Let me explain how hiring works here, because it involves quite a lot of people.",
         "It starts with the hiring manager, who writes the job description and sends it over to the recruiter.",
         "The recruiter posts the job and then screens the candidates, you know, a short phone call.",
         "The good ones go to the hiring manager for a technical interview.",
         "If the manager likes them, the candidate does a team interview with two engineers. If not, the recruiter "
         "sends a polite rejection.",
         "After the team interview the engineers give their feedback to the manager, and the manager decides.",
         "And then HR makes the offer and, if the candidate signs, HR does all the onboarding paperwork."),
        {"job description": r"job desc|description|\bjd\b", "post": r"post", "screen": r"screen|phone",
         "tech interview": r"technical", "team interview": r"team interview|engineers? interview",
         "rejection": r"reject", "decide": r"decid|decision", "offer": r"offer", "onboarding": r"onboard"},
        paths=(("job description", "post"), ("post", "screen"), ("screen", "tech interview"),
               ("tech interview", "team interview"), ("tech interview", "rejection"), ("team interview", "decide"),
               ("decide", "offer"), ("offer", "onboarding")),
        lanes={"job description": r"manager", "tech interview": r"manager", "decide": r"manager",
               "post": r"recruit", "screen": r"recruit", "rejection": r"recruit", "team interview": r"engineer|team",
               "offer": r"\bhr\b|human", "onboarding": r"\bhr\b|human"},
        max_nodes=12,
    ),
)}


def matches(view: dict, talk: Talk, concept: str) -> list[str]:
    pattern = re.compile(talk.concepts[concept], re.I)
    return [n["id"] for n in view["nodes"]
            if pattern.search(n["label"].strip()) or pattern.search(n["id"].replace("_", " "))]


def _reachable(view: dict, sources: list[str], targets: list[str]) -> bool:
    out: dict[str, list[str]] = {}
    for edge in view["edges"]:
        out.setdefault(edge["source"], []).append(edge["target"])
    seen, todo = set(sources), list(sources)
    while todo:
        for nxt in out.get(todo.pop(), []):
            if nxt not in seen:
                seen.add(nxt)
                todo.append(nxt)
    return bool(seen & (set(targets) - set(sources)))


def _direct(view: dict, sources: list[str], targets: list[str]) -> bool:
    return any(e["source"] in sources and e["target"] in targets and e["source"] != e["target"] for e in view["edges"])


def _lane_of(view: dict, node_id: str) -> str:
    return next((g["label"] for g in view["groups"] if node_id in g["members"]), "")


def evaluate(talk: Talk, view: dict) -> tuple[int, int, list[str]]:
    checks: list[tuple[str, bool]] = []
    found = {concept: matches(view, talk, concept) for concept in talk.concepts}
    checks += [(f"node {c}", bool(ids)) for c, ids in found.items()]
    checks += [(f"edge {a} -> {b}", _direct(view, found[a], found[b])) for a, b in talk.edges]
    checks += [(f"path {a} -> {b}", _reachable(view, found[a], found[b])) for a, b in talk.paths]
    checks += [(f"lane {c} in {lane}", any(re.search(lane, _lane_of(view, i), re.I) for i in found[c]))
               for c, lane in talk.lanes.items()]
    nodes = [n["id"] for n in view["nodes"]]
    linked = {e["source"] for e in view["edges"]} | {e["target"] for e in view["edges"]}
    labelled = sum(bool(e["label"]) for e in view["edges"])
    checks += [
        (f"<= {talk.max_nodes} nodes (has {len(nodes)})", 0 < len(nodes) <= talk.max_nodes),
        ("no orphan nodes", set(nodes) <= linked),
        ("no duplicate concept", all(len(found[c]) <= 1 for c in talk.unique)),
        (f"labelled edges >= {LABELLED_EDGES:.0%}", bool(view["edges"]) and labelled >= LABELLED_EDGES * len(view["edges"])),
        (f"kind in {talk.kinds} (is {view['kind']})", view["kind"] in talk.kinds),
    ]
    notes = [name for name, ok in checks if not ok]
    return sum(ok for _, ok in checks), len(checks), notes
