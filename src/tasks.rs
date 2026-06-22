use chrono::{Duration as ChronoDuration, Local};
use sqlx::{MySql, MySqlPool, Transaction};
use tokio::time::{Duration, interval};
use tracing::{info, warn};

const PENALTY_SCORE: i32 = 10;

/// 任务一：扫描并流转状态机（每 1 分钟执行一次）
/// 逻辑：将所有 PENDING 状态且超过 next_deadline 的预约，自动流转为 CANCELLED 违约状态
pub async fn start_deadline_checker(pool: MySqlPool) {
    // 设置定时器间隔为 1 分钟
    let mut interval = interval(Duration::from_secs(60));

    loop {
        interval.tick().await; // 等待下一个周期
        let current_time = Local::now().naive_local();

        let mut tx: Transaction<'_, MySql> = match pool.begin().await {
            Ok(t) => t,
            Err(e) => {
                warn!("[定时任务] 开启事务失败: {:?}", e);
                continue;
            }
        };

        // 批量流转状态机
        // PENDING/SUSPENDED -> CANCELLED
        // ACTIVE -> COMPLETED
        let update_result = sqlx::query!(
            r#"
            UPDATE reservations 
            SET 
                status = CASE 
                    WHEN status IN ('PENDING', 'SUSPENDED') THEN 'CANCELLED'
                    WHEN status = 'ACTIVE' THEN 'COMPLETED'
                    ELSE status
                END,
                next_deadline = ?
            WHERE status IN ('PENDING', 'ACTIVE', 'SUSPENDED') 
              AND next_deadline < ?
            "#,
            current_time,
            current_time
        )
        .execute(&mut *tx)
        .await;

        let rows_affected = match update_result {
            Ok(res) => res.rows_affected(),
            Err(e) => {
                warn!("[定时任务] 状态机流转更新失败: {:?}", e);
                let _ = tx.rollback().await;
                continue;
            }
        };

        // 如果本次没有需要处理的过期数据，直接提交事务，进入下一个周期
        if rows_affected == 0 {
            let _ = tx.commit().await;
            continue;
        }

        let score_result = sqlx::query!(
            r#"
            UPDATE users u
            SET u.score = GREATEST(u.score - ?, 0)
            WHERE u.id IN (
                SELECT r.user_id 
                FROM reservations r 
                WHERE r.status = 'CANCELLED' 
                  AND (r.note IS NULL OR r.note NOT LIKE '%PENALIZED%')
                  AND r.next_deadline < ?
            )
            "#,
            PENALTY_SCORE,
            current_time
        )
        .execute(&mut *tx)
        .await;

        if let Err(e) = score_result {
            warn!("[定时任务] 违约扣除用户积分失败: {:?}", e);
            let _ = tx.rollback().await; // 扣分失败则整个事务回滚
            continue;
        }

        // 4. 【核心防重步骤 2】扣分成功后，立刻为这批违约记录的 note 追加 '; PENALIZED' 标记
        let mark_result = sqlx::query!(
            r#"
            UPDATE reservations 
            SET note = COALESCE(CONCAT(note, 'PENALIZED'), 'PENALIZED')
            WHERE status = 'CANCELLED' 
              AND (note IS NULL OR note NOT LIKE '%PENALIZED%')
              AND next_deadline < ?
            "#,
            current_time
        )
        .execute(&mut *tx)
        .await;

        if let Err(e) = mark_result {
            warn!("[定时任务] 标记预约记录已扣分失败: {:?}", e);
            let _ = tx.rollback().await;
            continue;
        }

        // 5. 所有操作成功，提交事务
        if let Err(e) = tx.commit().await {
            warn!("[定时任务] 事务提交失败: {:?}", e);
        } else {
            info!("[定时任务] 成功处理了 {} 个超时预约", rows_affected);
        }
    }
}

/// 任务二：定期清理老旧数据（每 24 小时执行一次）
/// 逻辑：删除 30 天前的、已经是终态（COMPLETED 或 CANCELLED）的预约记录
pub async fn start_database_cleaner(pool: MySqlPool) {
    // 设置定时器间隔为 24 小时
    let mut interval = interval(Duration::from_secs(24 * 60 * 60));

    loop {
        interval.tick().await;

        // 计算 30 天前的时间点
        let border_date = Local::now().date_naive() - ChronoDuration::days(30);

        match sqlx::query!(
            r#"
            DELETE FROM reservations 
            WHERE date < ? AND status IN ('COMPLETED', 'CANCELLED')
            "#,
            border_date
        )
        .execute(&pool)
        .await
        {
            Ok(result) => {
                if result.rows_affected() > 0 {
                    info!("[定时任务] 清除了 {} 个过期记录", result.rows_affected());
                }
            }
            Err(_) => {
                warn!("[定时任务] 过期记录清除失败");
            }
        }
    }
}
