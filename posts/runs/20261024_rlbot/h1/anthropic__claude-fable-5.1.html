Looks like a scratch notes page for your RL bot project. There's no explicit question here, so a few quick thoughts on the todos in case they're useful:

1. **Split the actor/critic** — Usually the cleanest split is a shared `Model` wrapper that holds two independent `nn.Module`s (or equivalents) with separate optimizers, and a `save/load` that writes `actor.pt` / `critic.pt` (or one checkpoint with namespaced keys). That also makes item 2 easier since the zoo only ever needs the actor.

2. **Loadable model shapes in the zoo/tournament** — Yes, you'll probably need to keep old architecture definitions around, but you don't have to keep the *source files* frozen. Common approach: store an `arch` string/version + hyperparams (layer sizes, etc.) in the checkpoint metadata, and have a small registry that maps `arch` → constructor. Old shapes stay as tiny factory functions rather than whole duplicated modules. Deleting an arch = deleting its entry when no one needs those checkpoints anymore.

3. **Glow showing critic training** — if glow is your training dashboard, the critic usually just needs value loss + explained variance (`1 - Var(returns - values)/Var(returns)`) — the latter is the most informative single critic health metric.

4. **Train past duck for both** — depending on what "duck" is (baseline opponent? plateau?), worth logging win rate vs. duck as a fixed eval opponent separate from self-play so you can tell if the plateau is the policy or the critic.

Also, the `<div>test fasdf</div>` at the top is probably leftover debug text.

What are you actually after — help implementing one of these, or something else?
