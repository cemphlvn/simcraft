# Research: how ant colonies divide work and survive seasons

Collected for game 5 (`games/colony`), 2026-09-28. Four searches; what each model contributes to the game.

## Division of labour

| Mechanism | What the literature says | In the game |
|---|---|---|
| **Response thresholds** | Each worker has its own threshold for a task stimulus; low-threshold workers respond first, high-threshold ones only when the stimulus is strong. Varying thresholds spread workers over tasks without a leader. | Each ant is born with its own `threshold`; the stimulus is the nest's food shortage. |
| **Age polyethism** | Young workers stay inside and care for brood; older workers forage outside. Thresholds that change with age are one proposed mechanism. | Ants start as **Nurses** and become **Foragers** with age (earlier when food is short). |
| **Interaction rate** (Gordon, harvester ants) | Available foragers wait in an entrance chamber; each leaves when its recent rate of antennal contacts with *returning foragers carrying food* is high enough. Foraging rises when food is found quickly and falls when it is not, with no one counting anything. | Waiting foragers (**Ready**) go out when enough loaded foragers pass them near the entrance; a few **scouts** go anyway. |

Sources: [Scientific Reports: specialized castes or age-dependent switching](https://www.nature.com/articles/s41598-020-59920-5) · [PMC: age polyethism from social learning](https://pmc.ncbi.nlm.nih.gov/articles/PMC12396761/) · [arXiv: Polyethism in a colony of artificial ants](https://ar5iv.labs.arxiv.org/html/1104.3152) · [Behavioral Ecology: Interaction rate informs harvester ant task decisions](https://academic.oup.com/beheco/article/18/2/451/204082) · [PLOS One: Interactions increase forager availability](https://journals.plos.org/plosone/article?id=10.1371%2Fjournal.pone.0141971) · [PLOS Comp Biol: foraging as a closed-loop excitable system](https://journals.plos.org/ploscompbiol/article?id=10.1371%2Fjournal.pcbi.1006200)

## Stigmergy (coordination through the environment)

| Component | Literature | In the game |
|---|---|---|
| Deposit | An ant marks the cell it stands on. Models often use two pheromones (to food, to home). | Loaded foragers mark their way home with **food scent**. |
| Evaporation | Scent decays exponentially in time; without it, old trails to empty food never fade. | Every cell loses a percentage of its scent each tick. |
| Diffusion | Spreads scent to neighbours; too much diffusion flattens the gradient and trails cannot form. | Left out at first (optional in the models). |
| Gradient following | Ants sense neighbouring cells and move up the gradient, plus noise for exploration. | Searching ants step to the neighbouring cell with the most food scent, sometimes wander. |
| Path home | (Real ants also use path integration.) | Loaded ants walk straight home. |

Sources: [Decentralized foraging using only stigmergic communication](https://www.researchgate.net/publication/289667457_A_decentralized_ant_colony_foraging_model_using_only_stigmergic_communication) · [Stochastic model of trail following with two pheromones](https://arxiv.org/pdf/1508.06816) · [Continuous model of foraging with pheromones and trail formation](https://arxiv.org/pdf/1402.5611) · [Ant colony simulator (deposit, evaporation, diffusion, gradient)](https://starlighttools.org/science/ant-colony-simulator)

## Seasons

- Temperate colonies go **dormant in winter**: they cluster deep in the nest and cut their metabolism; larvae pause development.
- They **eat heavily in autumn**; granary (harvester) ants **store seeds** in chambers for the months when foraging is impossible.
- Activity falls below about 10–15 °C; day length is also a cue.

In the game: summer (bushes regrow), autumn (no regrowth, foraging still possible), winter (colony **Dormant**, eats little, no births, no regrowth). The colony survives winter on its store.

Sources: [Diapause in ants (guide for ant keepers)](https://www.poramorart.ca/ant-diapause-hibernation) · [antaddict: what happens to a colony in winter](https://antaddict.com/do-ants-hibernate/) · [Ant Shack: ants in winter](https://www.ant-shack.com/blogs/ant-articles/ants-in-winter-survival-strategies-of-these-cold-weather-warriors) · [bioRxiv: circannual cycles drive cold hardening in temperate ants](https://www.biorxiv.org/content/10.1101/2025.07.09.663877.full.pdf)
