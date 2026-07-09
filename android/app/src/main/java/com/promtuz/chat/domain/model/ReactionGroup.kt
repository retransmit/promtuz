package com.promtuz.chat.domain.model

/** One emoji's reactions on a message, aggregated. `mine` = the local user is among the reactors. */
data class ReactionGroup(val emoji: String, val count: Int, val mine: Boolean)
