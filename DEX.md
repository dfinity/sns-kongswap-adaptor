# What is a DEX?

A Decentralized EXchange (DEX) is a smart contract that enables users to trade tokens without relying on a centralized intermediary. Instead, trades are executed directly on-chain through liquidity pools.
Users can trade by selling one token in return for the another token.

# Liquidity Pool

Each DEX consists of one or more liquidity pools. A pool typically holds two tokens (say, $T_0$ and $T_1$). 
Roles of the users interacting with a liquidity pool can be:

1. Liquidity Providers (LPs): Users who deposit tokens into pools are called liquidity providers.
2. Traders: Users who swap one token for another. The exchange rate is determined by the ratio of tokens in the pool.
3. Platform providers: Owners of the DEX.


When a trade occurs, the trader pays a fee. This fee usually consists of:
1. A platform fee that goes to the DEX protocol, and
2. An LP fee that is distributed to liquidity providers, proportional to the amount of liquidity they contributed.

Some platforms allow LPs to claim and reinvest their fees, effectively compounding their rewards.

# Market Ratio

A liquidity pool maintains a market ratio (MR) between its two tokens:

$MR = \frac{reserve_0}{reserve_1}$

Equivalently, it is the price of $T_1$ quoted in $T_0$. Intuitively, the higher the relative abundance of $T_0$ compared to $T_1$, the cheaper $T_1$ becomes in terms of $T_0$.

When an LP deposits $amount_0$ and $amount_1$ to the pool (deposit ratio $DR = \frac{amount_0}{amount_1}$), the pool takes the following amounts from $T_0$ and $T_1$ respectively:

1. if $DR == MR$ => ($amount_0$, $amount_1$)
2. if $DR >  MR$ => ($amount_1 \times MR$, $amount_1$)
3. if $DR <  MR$ => ($amount_0$, $amount_0 \times MR$)

This ensures that liquidity is always added proportional to the current reserves' balances.
If $DR \ne MR$, the excess tokens are returned to the LP.

# Trading

Assume a trader wants to swap $T_0$ for $T_1$. To trade, they send $amount_0$ of $T_0$.

In the Automated Market Maker (AMM) model popularized by Uniswap V2, liquidity is defined as:

$L^2 = reserve_0 \times reserve_1$

This is called the *Constant Product Invariant*. The key idea is that, when trades happen, the product of reserves remains unchanged:

$reserve_0 \times reserve_1 = (reserve_0 + amount_0) \times (reserve_1 - amount_1)$

While this is the most common AMM model, other variants exist (e.g., constant sum for stablecoins, hybrid curves like Curve’s StableSwap).
The adaptor is, at this stage, implemented exclusively for Kongswap. Since Kongswap (as of 5th September 2025) employs the constant product AMM, our implementation and tests are aligned with that model.

$amount_1$ is the amount of $T_1$ that is going to be removed from the pool. Before sending
it to the user, some fees are deducted: a platform fee (paid to the DEX) and an LP fee (accrued to liquidity providers, proportional to their contribution).

# Withdrawing Liquidity

LPs can withdraw their tokens at any time. However, due to trades happening in between, they rarely receive back the same token amounts they initially deposited. Instead, they withdraw, depending on the net direction of trades:

1. More of one token,
2. Less of the other, and
3. The fees they have accrued during trading activity.

Some DEXes provide the LPs with the functionality to withdraw their accrued fee and, if desired, reinvest it to the protocol to compound their rewards.

# Pitfalls and Fallacies

In this section, we will highlight common pitfalls and misconceptions.

## Pool Price $\ne$ Actual Price

The pool exchange price (which is called *Local Price*) is solely defined by the ratio of assets in the pool. As users can trade their tokens, the ratio of the tokens diverges from the actual price, which is determined by the broader market of Centralized Exchanges, other DEXes, OTC desks, and oracles.

## Impermanent Loss

When LPs deposit assets to a pool, the exchange price can change according to the $MR$. As mentioned in [Pool Price $\ne$ Actual Price](#pool-price-actual-price), the local price can be different from the actual price. Hence, if one asset, e.g., $T_0$, is significantly more appreciated than the other one, liquidity providers end up selling their assets below the broader market value.

## Slippage

Traders don’t swap at a single fixed price, but along the curve defined by the *Constant Product Invariant*. Larger trades move the reserves more, so the average execution price is worse than the quoted start price.

Some DEXes implement a mechanism called Slippage Protection. When users trade or deposit, they can specify a maximum tolerable slippage (the maximum allowed deviation between the expected and actual execution price). If the trade would exceed this limit, the DEX cancels the execution and returns the tokens to the user.

## Front-running

When used as an extension to SNS, each initialization, deposit, or withdraw proposal goes through a public voting process. It means that attackers can guess the outcome of a voting when in its final stage. Then, they can make their transaction before the execution of the proposal (hence called front-running) and move the market ratio away from the ratio expected in the proposal. In this case, when the proposal gets executed, it observes a market ratio different than the expected market ratio. It means that the pool can be in an unhealthy state ($MR \gg 1$ or $MR \ll 1$) and in such cases if the SNS

1. deposits, the treasury funds also ends up in this unhealthy state
2. withdraws, as described in [Impermanent Loss](#impermanent-loss), it can lose money.

## LPs Risks

Although LPs are entitled to receive LP fees, LP fees do not always offset impermanent loss. As a result, LPs may end up with fewer assets in value than if they had simply held their tokens.

## Transaction Fees

Every deposit, withdrawal, or trade incurs ledger fees. These costs reduce the effective amount received. Fortunately, on ICP such fees are significantly lower than on most other blockchains.