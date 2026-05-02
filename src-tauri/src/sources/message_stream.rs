use chrono::{DateTime, Utc};

use super::{NormalizedRequest, NormalizedUsageEvent};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MessageTokenUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_write_input_tokens: i64,
}

impl MessageTokenUsage {
    fn has_any_usage(self) -> bool {
        self.input_tokens > 0
            || self.output_tokens > 0
            || self.cache_read_input_tokens > 0
            || self.cache_write_input_tokens > 0
    }

    fn total_tokens(self) -> i64 {
        if self.total_tokens > 0 {
            self.total_tokens
        } else {
            self.input_tokens + self.output_tokens
        }
    }
}

#[derive(Debug, Clone)]
pub struct MessageStreamItem {
    pub source_id: String,
    pub role: String,
    pub request_id: Option<String>,
    pub parent_id: Option<String>,
    pub status: Option<String>,
    pub model: Option<String>,
    pub usage: Option<MessageTokenUsage>,
    pub count_as_message: bool,
    pub source_created_at: Option<DateTime<Utc>>,
    pub source_updated_at: Option<DateTime<Utc>>,
    pub usage_event_time_utc: Option<DateTime<Utc>>,
    pub source_event_id: Option<String>,
    pub usage_event_granularity: Option<String>,
    pub usage_event_confidence: Option<String>,
    pub source_locator: String,
    pub use_as_request_locator: bool,
}

#[derive(Debug, Clone)]
pub struct MessageStreamAggregation {
    pub requests: Vec<NormalizedRequest>,
    pub events: Vec<NormalizedUsageEvent>,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
}

#[derive(Debug, Clone)]
pub struct MessageStreamAggregator {
    items: Vec<MessageStreamItem>,
    token_confidence: String,
    event_granularity: String,
}

impl MessageStreamAggregator {
    pub fn new(items: Vec<MessageStreamItem>) -> Self {
        Self {
            items,
            token_confidence: "high".to_string(),
            event_granularity: "request".to_string(),
        }
    }

    pub fn aggregate_parent_child_requests(&self) -> MessageStreamAggregation {
        let events = self.build_usage_events();
        let total_input_tokens = events.iter().map(|event| event.delta_input).sum();
        let total_output_tokens = events.iter().map(|event| event.delta_output).sum();

        MessageStreamAggregation {
            requests: self.build_parent_child_requests(),
            events,
            total_input_tokens,
            total_output_tokens,
        }
    }

    pub fn aggregate_assistant_request_groups(&self) -> MessageStreamAggregation {
        let events = self.build_usage_events();
        let total_input_tokens = events.iter().map(|event| event.delta_input).sum();
        let total_output_tokens = events.iter().map(|event| event.delta_output).sum();
        let groups = self.build_assistant_request_groups();

        MessageStreamAggregation {
            requests: groups
                .iter()
                .enumerate()
                .map(|(index, group)| self.build_request_from_group(index, group))
                .collect(),
            events,
            total_input_tokens,
            total_output_tokens,
        }
    }

    pub fn aggregate_sequential_user_requests(
        &self,
        generated_request_prefix: &str,
    ) -> MessageStreamAggregation {
        let groups = self.build_sequential_user_request_groups(generated_request_prefix);
        let requests = groups
            .iter()
            .enumerate()
            .map(|(index, group)| self.build_sequential_request(index, group))
            .collect::<Vec<_>>();
        let events = groups
            .iter()
            .zip(requests.iter())
            .filter_map(|(group, request)| self.build_group_usage_event(group, request))
            .collect::<Vec<_>>();
        let total_input_tokens = events.iter().map(|event| event.delta_input).sum();
        let total_output_tokens = events.iter().map(|event| event.delta_output).sum();

        MessageStreamAggregation {
            requests,
            events,
            total_input_tokens,
            total_output_tokens,
        }
    }

    pub fn aggregate_sequential_user_requests_with_item_events(
        &self,
        generated_request_prefix: &str,
    ) -> MessageStreamAggregation {
        let groups = self.build_sequential_user_request_groups(generated_request_prefix);
        let requests = groups
            .iter()
            .enumerate()
            .map(|(index, group)| self.build_sequential_request(index, group))
            .collect::<Vec<_>>();
        let events = self.build_usage_events();
        let total_input_tokens = events.iter().map(|event| event.delta_input).sum();
        let total_output_tokens = events.iter().map(|event| event.delta_output).sum();

        MessageStreamAggregation {
            requests,
            events,
            total_input_tokens,
            total_output_tokens,
        }
    }

    pub fn aggregate_explicit_request_groups(&self) -> MessageStreamAggregation {
        let groups = self.build_explicit_request_groups();
        let requests = groups
            .iter()
            .enumerate()
            .map(|(index, group)| self.build_explicit_request(index, group))
            .collect::<Vec<_>>();
        let events = groups
            .iter()
            .zip(requests.iter())
            .filter_map(|(group, request)| self.build_group_usage_event(group, request))
            .collect::<Vec<_>>();
        let total_input_tokens = events.iter().map(|event| event.delta_input).sum();
        let total_output_tokens = events.iter().map(|event| event.delta_output).sum();

        MessageStreamAggregation {
            requests,
            events,
            total_input_tokens,
            total_output_tokens,
        }
    }

    pub fn aggregate_explicit_request_groups_with_item_events(&self) -> MessageStreamAggregation {
        let groups = self.build_explicit_request_groups();
        let requests = groups
            .iter()
            .enumerate()
            .map(|(index, group)| self.build_explicit_request(index, group))
            .collect::<Vec<_>>();
        let events = self.build_usage_events();
        let total_input_tokens = events.iter().map(|event| event.delta_input).sum();
        let total_output_tokens = events.iter().map(|event| event.delta_output).sum();

        MessageStreamAggregation {
            requests,
            events,
            total_input_tokens,
            total_output_tokens,
        }
    }

    fn build_usage_events(&self) -> Vec<NormalizedUsageEvent> {
        self.items
            .iter()
            .filter(|message| message.role == "assistant")
            .filter_map(|message| {
                let usage = message.usage?;
                if !usage.has_any_usage() {
                    return None;
                }

                Some(NormalizedUsageEvent {
                    event_time_utc: message.usage_event_time_utc?,
                    model: message.model.clone(),
                    delta_input: usage.input_tokens,
                    delta_output: usage.output_tokens,
                    delta_total: usage.total_tokens(),
                    cache_read_input_tokens: usage.cache_read_input_tokens,
                    cache_write_input_tokens: usage.cache_write_input_tokens,
                    source_event_id: message
                        .source_event_id
                        .clone()
                        .or_else(|| Some(message.source_id.clone())),
                    granularity: message
                        .usage_event_granularity
                        .clone()
                        .unwrap_or_else(|| self.event_granularity.clone()),
                    confidence: message
                        .usage_event_confidence
                        .clone()
                        .unwrap_or_else(|| self.token_confidence.clone()),
                })
            })
            .collect()
    }

    fn build_parent_child_requests(&self) -> Vec<NormalizedRequest> {
        self.items
            .iter()
            .filter(|message| message.role == "user")
            .enumerate()
            .map(|(index, user_message)| {
                let child_messages = self.child_assistant_messages(&user_message.source_id);
                let usage = child_messages
                    .iter()
                    .filter_map(|message| message.usage)
                    .fold(MessageTokenUsage::default(), |mut total, usage| {
                        total.input_tokens += usage.input_tokens;
                        total.output_tokens += usage.output_tokens;
                        total.total_tokens += usage.total_tokens();
                        total.cache_read_input_tokens += usage.cache_read_input_tokens;
                        total.cache_write_input_tokens += usage.cache_write_input_tokens;
                        total
                    });
                let source_updated_at =
                    newest_message_timestamp(&child_messages).or(user_message.source_updated_at);
                let model = child_messages
                    .iter()
                    .filter_map(|message| message.model.clone())
                    .next_back()
                    .or_else(|| user_message.model.clone());
                let status = child_messages
                    .iter()
                    .filter_map(|message| message.status.clone())
                    .next_back();

                NormalizedRequest {
                    source_request_id: Some(user_message.source_id.clone()),
                    sequence_no: index as i64 + 1,
                    status,
                    message_count: child_messages.len() as i64 + 1,
                    model,
                    input_tokens: Some(usage.input_tokens),
                    output_tokens: Some(usage.output_tokens),
                    total_tokens: Some(usage.total_tokens()),
                    cache_read_input_tokens: Some(usage.cache_read_input_tokens),
                    cache_write_input_tokens: Some(usage.cache_write_input_tokens),
                    token_confidence: Some(self.token_confidence.clone()),
                    source_created_at: user_message.source_created_at,
                    source_updated_at,
                    source_locator: user_message.source_locator.clone(),
                }
            })
            .collect()
    }

    fn child_assistant_messages(&self, user_message_id: &str) -> Vec<&MessageStreamItem> {
        self.items
            .iter()
            .filter(|message| {
                message.role == "assistant" && message.parent_id.as_deref() == Some(user_message_id)
            })
            .collect()
    }

    fn build_assistant_request_groups(&self) -> Vec<MessageRequestGroup<'_>> {
        let mut groups = Vec::<MessageRequestGroup<'_>>::new();
        for message in self
            .items
            .iter()
            .filter(|message| message.role == "assistant")
        {
            let request_id = message
                .request_id
                .clone()
                .or_else(|| message.parent_id.clone())
                .unwrap_or_else(|| message.source_id.clone());
            let group_index = groups
                .iter()
                .position(|group| group.source_request_id == request_id)
                .unwrap_or_else(|| {
                    let user_message = self
                        .items
                        .iter()
                        .find(|item| item.role == "user" && item.source_id == request_id);
                    groups.push(MessageRequestGroup {
                        source_request_id: request_id.clone(),
                        user_message,
                        assistant_messages: Vec::new(),
                    });
                    groups.len() - 1
                });

            groups[group_index].assistant_messages.push(message);
        }

        groups
    }

    fn build_request_from_group(
        &self,
        index: usize,
        group: &MessageRequestGroup<'_>,
    ) -> NormalizedRequest {
        let usage = group
            .assistant_messages
            .iter()
            .filter_map(|message| message.usage)
            .fold(MessageTokenUsage::default(), |mut total, usage| {
                total.input_tokens += usage.input_tokens;
                total.output_tokens += usage.output_tokens;
                total.total_tokens += usage.total_tokens();
                total.cache_read_input_tokens += usage.cache_read_input_tokens;
                total.cache_write_input_tokens += usage.cache_write_input_tokens;
                total
            });
        let has_usage = usage.has_any_usage();
        let source_created_at = group
            .user_message
            .and_then(|message| message.source_created_at)
            .or_else(|| {
                group
                    .assistant_messages
                    .first()
                    .and_then(|message| message.source_created_at)
            });
        let source_updated_at =
            newest_message_timestamp(&group.assistant_messages).or(source_created_at);
        let model = group
            .assistant_messages
            .iter()
            .filter_map(|message| message.model.clone())
            .next_back()
            .or_else(|| group.user_message.and_then(|message| message.model.clone()));
        let status = group
            .assistant_messages
            .iter()
            .filter_map(|message| message.status.clone())
            .next_back();
        let locator_message = group
            .assistant_messages
            .iter()
            .copied()
            .rev()
            .find(|message| message.use_as_request_locator)
            .or_else(|| group.assistant_messages.first().copied());
        let source_locator = locator_message
            .map(|message| message.source_locator.clone())
            .or_else(|| {
                group
                    .user_message
                    .map(|message| message.source_locator.clone())
            })
            .unwrap_or_default();

        NormalizedRequest {
            source_request_id: Some(group.source_request_id.clone()),
            sequence_no: index as i64 + 1,
            status,
            message_count: group.assistant_messages.len() as i64
                + if group.user_message.is_some() { 1 } else { 0 },
            model,
            input_tokens: has_usage.then_some(usage.input_tokens),
            output_tokens: has_usage.then_some(usage.output_tokens),
            total_tokens: has_usage.then_some(usage.total_tokens()),
            cache_read_input_tokens: has_usage.then_some(usage.cache_read_input_tokens),
            cache_write_input_tokens: has_usage.then_some(usage.cache_write_input_tokens),
            token_confidence: has_usage.then_some(self.token_confidence.clone()),
            source_created_at,
            source_updated_at,
            source_locator,
        }
    }

    fn build_sequential_user_request_groups(
        &self,
        generated_request_prefix: &str,
    ) -> Vec<SequentialRequestGroup<'_>> {
        let mut groups = Vec::<SequentialRequestGroup<'_>>::new();

        for message in &self.items {
            if message.role == "user" || groups.is_empty() {
                let sequence_no = groups.len() as i64 + 1;
                let source_request_id = message.request_id.clone().unwrap_or_else(|| {
                    if message.role == "user" {
                        message.source_id.clone()
                    } else {
                        format!("{generated_request_prefix}-{sequence_no}")
                    }
                });
                groups.push(SequentialRequestGroup {
                    source_request_id,
                    messages: Vec::new(),
                });
            }

            if let Some(group) = groups.last_mut() {
                group.messages.push(message);
            }
        }

        groups
    }

    fn build_explicit_request_groups(&self) -> Vec<SequentialRequestGroup<'_>> {
        let mut groups = Vec::<SequentialRequestGroup<'_>>::new();

        for message in &self.items {
            let source_request_id = message
                .request_id
                .clone()
                .unwrap_or_else(|| message.source_id.clone());
            let should_start_group = groups
                .last()
                .map(|group| group.source_request_id != source_request_id)
                .unwrap_or(true);

            if should_start_group {
                groups.push(SequentialRequestGroup {
                    source_request_id,
                    messages: Vec::new(),
                });
            }

            if let Some(group) = groups.last_mut() {
                group.messages.push(message);
            }
        }

        groups
    }

    fn build_explicit_request(
        &self,
        index: usize,
        group: &SequentialRequestGroup<'_>,
    ) -> NormalizedRequest {
        let usage = group
            .messages
            .iter()
            .filter_map(|message| message.usage)
            .fold(MessageTokenUsage::default(), |mut total, usage| {
                total.input_tokens += usage.input_tokens;
                total.output_tokens += usage.output_tokens;
                total.total_tokens += usage.total_tokens();
                total.cache_read_input_tokens += usage.cache_read_input_tokens;
                total.cache_write_input_tokens += usage.cache_write_input_tokens;
                total
            });
        let has_usage = usage.has_any_usage();
        let first_message = group.messages.first().copied();
        let source_created_at = group
            .messages
            .iter()
            .filter_map(|message| message.source_created_at)
            .min()
            .or_else(|| first_message.and_then(|message| message.source_created_at));
        let source_updated_at = newest_message_timestamp(&group.messages).or(source_created_at);
        let model = group
            .messages
            .iter()
            .filter_map(|message| message.model.clone())
            .next_back();
        let status = group
            .messages
            .iter()
            .filter_map(|message| message.status.clone())
            .next_back();
        let source_locator = first_message
            .map(|message| message.source_locator.clone())
            .unwrap_or_default();

        NormalizedRequest {
            source_request_id: Some(group.source_request_id.clone()),
            sequence_no: index as i64 + 1,
            status,
            message_count: group
                .messages
                .iter()
                .filter(|message| message.count_as_message)
                .count() as i64,
            model,
            input_tokens: has_usage.then_some(usage.input_tokens),
            output_tokens: has_usage.then_some(usage.output_tokens),
            total_tokens: has_usage.then_some(usage.total_tokens()),
            cache_read_input_tokens: has_usage.then_some(usage.cache_read_input_tokens),
            cache_write_input_tokens: has_usage.then_some(usage.cache_write_input_tokens),
            token_confidence: has_usage.then_some(self.token_confidence.clone()),
            source_created_at,
            source_updated_at,
            source_locator,
        }
    }

    fn build_sequential_request(
        &self,
        index: usize,
        group: &SequentialRequestGroup<'_>,
    ) -> NormalizedRequest {
        let usage = group
            .messages
            .iter()
            .filter_map(|message| message.usage)
            .fold(MessageTokenUsage::default(), |mut total, usage| {
                total.input_tokens += usage.input_tokens;
                total.output_tokens += usage.output_tokens;
                total.total_tokens += usage.total_tokens();
                total.cache_read_input_tokens += usage.cache_read_input_tokens;
                total.cache_write_input_tokens += usage.cache_write_input_tokens;
                total
            });
        let has_usage = usage.has_any_usage();
        let first_message = group.messages.first().copied();
        let source_created_at = group
            .messages
            .iter()
            .filter_map(|message| message.source_created_at)
            .min()
            .or_else(|| first_message.and_then(|message| message.source_created_at));
        let source_updated_at = newest_message_timestamp(&group.messages).or(source_created_at);
        let model = group
            .messages
            .iter()
            .filter_map(|message| message.model.clone())
            .next_back();
        let status = group
            .messages
            .iter()
            .filter_map(|message| message.status.clone())
            .next_back()
            .or_else(|| Some("completed".to_string()));
        let source_locator = first_message
            .map(|message| message.source_locator.clone())
            .unwrap_or_default();

        NormalizedRequest {
            source_request_id: Some(group.source_request_id.clone()),
            sequence_no: index as i64 + 1,
            status,
            message_count: group
                .messages
                .iter()
                .filter(|message| message.count_as_message)
                .count() as i64,
            model,
            input_tokens: has_usage.then_some(usage.input_tokens),
            output_tokens: has_usage.then_some(usage.output_tokens),
            total_tokens: has_usage.then_some(usage.total_tokens()),
            cache_read_input_tokens: Some(usage.cache_read_input_tokens),
            cache_write_input_tokens: Some(usage.cache_write_input_tokens),
            token_confidence: has_usage.then_some(self.token_confidence.clone()),
            source_created_at,
            source_updated_at,
            source_locator,
        }
    }

    fn build_group_usage_event(
        &self,
        group: &SequentialRequestGroup<'_>,
        request: &NormalizedRequest,
    ) -> Option<NormalizedUsageEvent> {
        let delta_input = request.input_tokens?;
        let delta_output = request.output_tokens?;
        if delta_input <= 0 && delta_output <= 0 {
            return None;
        }

        let event_time_utc = request.source_updated_at.or(request.source_created_at)?;
        let source_event_id = group
            .messages
            .iter()
            .rev()
            .find(|message| message.usage.is_some())
            .and_then(|message| {
                message
                    .source_event_id
                    .clone()
                    .or_else(|| Some(message.source_id.clone()))
            })
            .or_else(|| request.source_request_id.clone());

        Some(NormalizedUsageEvent {
            event_time_utc,
            model: request.model.clone(),
            delta_input,
            delta_output,
            delta_total: request.total_tokens.unwrap_or(delta_input + delta_output),
            cache_read_input_tokens: request.cache_read_input_tokens.unwrap_or(0),
            cache_write_input_tokens: request.cache_write_input_tokens.unwrap_or(0),
            source_event_id,
            granularity: self.event_granularity.clone(),
            confidence: request
                .token_confidence
                .clone()
                .unwrap_or_else(|| self.token_confidence.clone()),
        })
    }
}

#[derive(Debug, Clone)]
struct MessageRequestGroup<'a> {
    source_request_id: String,
    user_message: Option<&'a MessageStreamItem>,
    assistant_messages: Vec<&'a MessageStreamItem>,
}

#[derive(Debug, Clone)]
struct SequentialRequestGroup<'a> {
    source_request_id: String,
    messages: Vec<&'a MessageStreamItem>,
}

fn newest_message_timestamp(messages: &[&MessageStreamItem]) -> Option<DateTime<Utc>> {
    messages
        .iter()
        .filter_map(|message| message.source_updated_at)
        .max()
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn ts(second: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 23, 10, 0, second)
            .single()
            .expect("valid test timestamp")
    }

    fn item(source_id: &str, role: &str, parent_id: Option<&str>) -> MessageStreamItem {
        MessageStreamItem {
            source_id: source_id.to_string(),
            role: role.to_string(),
            request_id: None,
            parent_id: parent_id.map(ToString::to_string),
            status: None,
            model: None,
            usage: None,
            count_as_message: true,
            source_created_at: Some(ts(0)),
            source_updated_at: Some(ts(0)),
            usage_event_time_utc: Some(ts(0)),
            source_event_id: None,
            usage_event_granularity: None,
            usage_event_confidence: None,
            source_locator: format!("locator:{source_id}"),
            use_as_request_locator: false,
        }
    }

    #[test]
    fn groups_parent_child_messages_into_requests_and_usage_events() {
        let user = item("user-1", "user", None);
        let mut assistant = item("assistant-1", "assistant", Some("user-1"));
        assistant.status = Some("done".to_string());
        assistant.model = Some("model-a".to_string());
        assistant.usage = Some(MessageTokenUsage {
            input_tokens: 100,
            output_tokens: 30,
            total_tokens: 130,
            cache_read_input_tokens: 10,
            cache_write_input_tokens: 5,
        });
        assistant.source_updated_at = Some(ts(5));
        assistant.usage_event_time_utc = Some(ts(6));

        let aggregate =
            MessageStreamAggregator::new(vec![user, assistant]).aggregate_parent_child_requests();

        assert_eq!(aggregate.requests.len(), 1);
        assert_eq!(aggregate.events.len(), 1);
        assert_eq!(aggregate.total_input_tokens, 100);
        assert_eq!(aggregate.total_output_tokens, 30);

        let request = &aggregate.requests[0];
        assert_eq!(request.source_request_id.as_deref(), Some("user-1"));
        assert_eq!(request.message_count, 2);
        assert_eq!(request.model.as_deref(), Some("model-a"));
        assert_eq!(request.status.as_deref(), Some("done"));
        assert_eq!(request.input_tokens, Some(100));
        assert_eq!(request.output_tokens, Some(30));
        assert_eq!(request.total_tokens, Some(130));
        assert_eq!(request.cache_read_input_tokens, Some(10));
        assert_eq!(request.cache_write_input_tokens, Some(5));

        let event = &aggregate.events[0];
        assert_eq!(event.source_event_id.as_deref(), Some("assistant-1"));
        assert_eq!(event.delta_input, 100);
        assert_eq!(event.delta_output, 30);
        assert_eq!(event.delta_total, 130);
        assert_eq!(event.granularity, "request");
        assert_eq!(event.confidence, "high");
    }

    #[test]
    fn skips_zero_usage_events_but_keeps_zero_token_requests() {
        let user = item("user-1", "user", None);
        let mut assistant = item("assistant-1", "assistant", Some("user-1"));
        assistant.usage = Some(MessageTokenUsage::default());

        let aggregate =
            MessageStreamAggregator::new(vec![user, assistant]).aggregate_parent_child_requests();

        assert_eq!(aggregate.requests.len(), 1);
        assert!(aggregate.events.is_empty());
        assert_eq!(aggregate.requests[0].input_tokens, Some(0));
        assert_eq!(aggregate.requests[0].output_tokens, Some(0));
        assert_eq!(aggregate.requests[0].total_tokens, Some(0));
    }

    #[test]
    fn groups_assistants_by_resolved_request_id_and_preserves_part_locator() {
        let user = item("user-1", "user", None);
        let mut assistant = item("assistant-1", "assistant", None);
        assistant.request_id = Some("user-1".to_string());
        assistant.source_event_id = Some("part-1".to_string());
        assistant.source_locator = "locator:part-1".to_string();
        assistant.use_as_request_locator = true;
        assistant.usage = Some(MessageTokenUsage {
            input_tokens: 20,
            output_tokens: 7,
            total_tokens: 31,
            cache_read_input_tokens: 2,
            cache_write_input_tokens: 2,
        });

        let aggregate = MessageStreamAggregator::new(vec![user, assistant])
            .aggregate_assistant_request_groups();

        assert_eq!(aggregate.requests.len(), 1);
        assert_eq!(aggregate.requests[0].source_locator, "locator:part-1");
        assert_eq!(aggregate.requests[0].total_tokens, Some(31));
        assert_eq!(
            aggregate.events[0].source_event_id.as_deref(),
            Some("part-1")
        );
        assert_eq!(aggregate.events[0].delta_total, 31);
    }

    #[test]
    fn groups_sequential_messages_between_user_turns() {
        let first_user = item("user-1", "user", None);
        let mut first_assistant = item("assistant-1", "assistant", None);
        first_assistant.usage = Some(MessageTokenUsage {
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            cache_read_input_tokens: 0,
            cache_write_input_tokens: 0,
        });
        first_assistant.source_event_id = Some("usage-1".to_string());
        first_assistant.source_updated_at = Some(ts(5));
        let second_user = item("user-2", "user", None);

        let aggregate =
            MessageStreamAggregator::new(vec![first_user, first_assistant, second_user])
                .aggregate_sequential_user_requests("generated");

        assert_eq!(aggregate.requests.len(), 2);
        assert_eq!(
            aggregate.requests[0].source_request_id.as_deref(),
            Some("user-1")
        );
        assert_eq!(aggregate.requests[0].message_count, 2);
        assert_eq!(aggregate.requests[0].input_tokens, Some(10));
        assert_eq!(
            aggregate.requests[1].source_request_id.as_deref(),
            Some("user-2")
        );
        assert_eq!(aggregate.requests[1].token_confidence, None);
        assert_eq!(aggregate.events.len(), 1);
        assert_eq!(
            aggregate.events[0].source_event_id.as_deref(),
            Some("usage-1")
        );
    }

    #[test]
    fn sequential_item_events_keep_event_granularity_and_skip_message_count() {
        let user = item("user-1", "user", None);
        let assistant = item("assistant-1", "assistant", None);
        let mut usage = item("usage-1", "assistant", None);
        usage.count_as_message = false;
        usage.usage_event_granularity = Some("snapshot_delta".to_string());
        usage.usage_event_confidence = Some("medium".to_string());
        usage.usage = Some(MessageTokenUsage {
            input_tokens: 8,
            output_tokens: 3,
            total_tokens: 11,
            cache_read_input_tokens: 1,
            cache_write_input_tokens: 0,
        });

        let aggregate = MessageStreamAggregator::new(vec![user, assistant, usage])
            .aggregate_sequential_user_requests_with_item_events("generated");

        assert_eq!(aggregate.requests.len(), 1);
        assert_eq!(aggregate.requests[0].message_count, 2);
        assert_eq!(aggregate.requests[0].input_tokens, Some(8));
        assert_eq!(aggregate.events.len(), 1);
        assert_eq!(aggregate.events[0].granularity, "snapshot_delta");
        assert_eq!(aggregate.events[0].confidence, "medium");
    }

    #[test]
    fn groups_explicit_request_ids_without_splitting_repeated_user_messages() {
        let mut first_user = item("user-1", "user", None);
        first_user.request_id = Some("request-1".to_string());
        first_user.status = Some("open".to_string());
        let mut second_user = item("user-2", "user", None);
        second_user.request_id = Some("request-1".to_string());
        let mut assistant = item("assistant-1", "assistant", None);
        assistant.request_id = Some("request-1".to_string());
        assistant.status = Some("completed".to_string());
        assistant.usage = Some(MessageTokenUsage {
            input_tokens: 12,
            output_tokens: 5,
            total_tokens: 17,
            cache_read_input_tokens: 0,
            cache_write_input_tokens: 0,
        });

        let aggregate = MessageStreamAggregator::new(vec![first_user, second_user, assistant])
            .aggregate_explicit_request_groups();

        assert_eq!(aggregate.requests.len(), 1);
        assert_eq!(
            aggregate.requests[0].source_request_id.as_deref(),
            Some("request-1")
        );
        assert_eq!(aggregate.requests[0].message_count, 3);
        assert_eq!(aggregate.requests[0].status.as_deref(), Some("completed"));
        assert_eq!(aggregate.events.len(), 1);
        assert_eq!(aggregate.events[0].delta_total, 17);
    }

    #[test]
    fn explicit_request_groups_can_keep_item_level_usage_events() {
        let mut user = item("user-1", "user", None);
        user.request_id = Some("request-1".to_string());
        let mut first_assistant = item("assistant-1", "assistant", None);
        first_assistant.request_id = Some("request-1".to_string());
        first_assistant.usage = Some(MessageTokenUsage {
            input_tokens: 10,
            output_tokens: 4,
            total_tokens: 14,
            cache_read_input_tokens: 0,
            cache_write_input_tokens: 0,
        });
        let mut second_assistant = item("assistant-2", "assistant", None);
        second_assistant.request_id = Some("request-1".to_string());
        second_assistant.usage = Some(MessageTokenUsage {
            input_tokens: 7,
            output_tokens: 3,
            total_tokens: 10,
            cache_read_input_tokens: 0,
            cache_write_input_tokens: 0,
        });

        let aggregate = MessageStreamAggregator::new(vec![user, first_assistant, second_assistant])
            .aggregate_explicit_request_groups_with_item_events();

        assert_eq!(aggregate.requests.len(), 1);
        assert_eq!(aggregate.requests[0].input_tokens, Some(17));
        assert_eq!(aggregate.events.len(), 2);
        assert_eq!(aggregate.events[0].delta_total, 14);
        assert_eq!(aggregate.events[1].delta_total, 10);
    }
}
