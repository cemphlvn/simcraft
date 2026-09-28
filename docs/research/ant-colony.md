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

## How ants sense the seasons (added for the spring gap, colony step 013+)

The principle for the game: **an ant never reads the season; it senses its own surroundings and keeps its own time.**

| Finding | Literature | In the game |
|---|---|---|
| Temperature is sensed locally | Ant antennae are highly temperature sensitive; they express thermosensitive TRP channels (Atta texana: a hymenopteran-specific TRPA channel activated by heat and cold) | Each ant senses the warmth *where it is*; the nest is buffered against the outside |
| Seasons come from an inner timer | Most temperate ants' seasonal development depends on an **endogenous timer** (processes endogenous to the colony), with temperature playing a *corrective* role: lower temperatures accelerate diapause onset, higher ones delay it | The colony keeps its own year-clock; cold nudges it towards dormancy |
| Day length is rarely used | Photoperiodic control of diapause is "unexpectedly uncommon among ants"; *Myrmica* is the exception | No day-length sense (a possible species variant later) |
| Workers carry the cue | Larvae cannot sense photoperiod; workers' physiological state regulates larval development | Brood follows the workers, not the environment |
| Lag and reversibility | Diapause can start while it is still warm, and can be ended and induced again "each time after a little delay" | Entering and leaving dormancy takes sustained cues, not one cold tick |
| Metabolism follows body temperature | Respiration rises ~60 % from 24 to 30 °C in fire ants, a Q10 of about 2; cold-adapted species differ | An ant's burn rate follows its (sensed) body warmth |

Sources: [IntechOpen: seasonal cycles and strategies in ants](https://www.intechopen.com/chapters/60505) · [Temperature and photoperiodic control of diapause induction in Lepisiota semenovi](https://pubmed.ncbi.nlm.nih.gov/19435261/) · [Insect Diapause: interpreting seasonal cues](https://www.cambridge.org/core/books/abs/insect-diapause/interpreting-seasonal-cues-to-program-diapause-entry/4FF16B44A23E596C62DFDD5B3FC50C0C) · [Heat- and cold-activated TRPA channel in Atta texana (PMC)](https://pmc.ncbi.nlm.nih.gov/articles/PMC11824891/) · [Standard metabolic rate of the fire ant: temperature, mass, caste](https://www.sciencedirect.com/science/article/abs/pii/S0022191099000360) · [Cold comfort: metabolic rate and low-temperature tolerance in ants (PMC)](https://pmc.ncbi.nlm.nih.gov/articles/PMC10510448/) · [bioRxiv: circannual cycles drive seasonal cold hardening in temperate ants](https://www.biorxiv.org/content/10.1101/2025.07.09.663877.full.pdf)

## What warmth gives an ant colony (colony steps 015–017)

| Finding | Literature | In the game |
|---|---|---|
| Nests are kept warm, actively | Red wood ants keep their nests above 20 °C in spring and summer, "which enables faster development of their brood"; mounds act as solar collectors, workers add metabolic heat (in early spring burning lipid reserves to hold ~30 °C), and ants regulate by moving aggregations and ventilating | The nest has a climate (insulation, solar gain, workers' heat); ants can rebuild thatch |
| Brood develops faster when warm | as above | Brood is raised faster in a warm nest (lay chance scales with the warmth nurses feel) |
| Walking speed rises with temperature | Walking speed is positively related to temperature: ~0.1 cm/s per °C (winter ant, 6–24 °C), ~0.2 cm/s per °C in Argentine ants at 25–34 °C | Cold ants move less often, in proportion to the warmth they feel |

Sources: [Thermoregulation strategies in ants, focus on red wood ants (PMC)](https://www.ncbi.nlm.nih.gov/pmc/articles/PMC3962001/) · [Respiration in wood ant nests across seasons](https://www.sciencedirect.com/science/article/abs/pii/S003807171500125X) · [Temperature-dependent walking speed: winter ant vs Argentine ant](https://www.sciencedirect.com/science/article/abs/pii/S0306456522002066) · [Thermal limits (Stanford PDF)](https://web.stanford.edu/~dmgordon/articles/doi/10.1016-j.jtherbio.2022.103392/Thermal%20limits.pdf) · [Foraging and locomotion of Argentine ants from winter aggregations (PMC)](https://pmc.ncbi.nlm.nih.gov/articles/PMC6084982/) · [Temperature and foraging schedules in Myrmecia (JEB)](https://journals.biologists.com/jeb/article/214/16/2730/10378/Different-effects-of-temperature-on-foraging)
