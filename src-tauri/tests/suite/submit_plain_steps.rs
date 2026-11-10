//! The sentence `submit_queue` answers when it stops an upload because the
//! current steps of its updates could not be read (backlog A1.9). The
//! screen recognises it by its exact text (QueueSection.tsx), so both sides
//! pin the same words.

#[test]
fn the_plain_steps_sentence_is_word_for_word_the_one_the_screen_knows() {
    assert_eq!(
        v2_lib::commands::queue::PLAIN_STEPS_REFUSAL,
        "The current steps of the cases this upload updates could not be read, so nothing was sent. \
         If you upload anyway, their steps are written as plain text and any formatting or pictures in them are lost."
    );
}
